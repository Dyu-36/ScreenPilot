#[cfg(target_os = "windows")]
mod imp {
    use anyhow::{anyhow, Result};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::{
            CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject,
            EVENT_MODIFY_STATE, INFINITE,
        },
    };

    const MUTEX_NAME: &str = "Local\\TackAgent.SingleInstance.v2";
    const SHOW_EVENT_NAME: &str = "Local\\TackAgent.ShowEvent.v1";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Khởi tạo single-instance. Trả về `Ok(false)` nếu đã có tiến trình khác đang chạy.
    /// Mutex handle không bao giờ được đóng — tiến trình sống đến khi bị kill,
    /// OS sẽ tự dọn dẹp khi tiến trình kết thúc.
    pub fn acquire_single_instance(on_show: impl Fn() + Send + 'static) -> Result<bool> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(anyhow!("không tạo được khóa single-instance của Tack Agent"));
        }

        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return Ok(false);
        }

        // Thread nghe sự kiện "mở lại cửa sổ" từ tiến trình thứ hai.
        std::thread::spawn(move || {
            let name = wide(SHOW_EVENT_NAME);
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
            if event.is_null() {
                return;
            }
            loop {
                unsafe { WaitForSingleObject(event, INFINITE) };
                on_show();
            }
        });

        Ok(true)
    }

    /// Được tiến trình thứ hai gọi để yêu cầu instance đang chạy mở lại cửa sổ.
    pub fn notify_show() -> bool {
        let name = wide(SHOW_EVENT_NAME);
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if event.is_null() {
            return false;
        }
        let signaled = unsafe { SetEvent(event) };
        unsafe {
            CloseHandle(event);
        }
        signaled != 0
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use std::{
        fs,
        io::{Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::PathBuf,
    };

    use anyhow::{Context, Result};

    use crate::state;

    fn socket_path() -> PathBuf {
        state::config_dir().join("agent.sock")
    }

    /// Khởi tạo single-instance bằng cách bind unix socket.
    /// Trả về `Ok(false)` nếu đã có tiến trình khác đang chạy.
    pub fn acquire_single_instance(on_show: impl Fn() + Send + 'static) -> Result<bool> {
        let path = socket_path();
        let _ = fs::create_dir_all(state::config_dir());

        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                // Socket cũ còn sống hay còn sót lại từ lần chạy trước?
                if UnixStream::connect(&path).is_ok() {
                    return Ok(false);
                }
                let _ = fs::remove_file(&path);
                match UnixListener::bind(&path) {
                    Ok(listener) => listener,
                    Err(_) => return Ok(false),
                }
            }
            Err(err) => {
                return Err(err).context("không tạo được khóa single-instance của Tack Agent")
            }
        };

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut buffer = [0u8; 16];
                let mut stream = stream;
                match stream.read(&mut buffer) {
                    Ok(_) => on_show(),
                    Err(_) => continue,
                }
            }
        });

        Ok(true)
    }

    pub fn notify_show() -> bool {
        let Ok(mut stream) = UnixStream::connect(socket_path()) else {
            return false;
        };
        stream.write_all(b"show").is_ok()
    }
}

pub use imp::*;
