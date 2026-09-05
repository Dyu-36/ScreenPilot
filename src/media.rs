use std::{path::Path, time::Duration};

use anyhow::{bail, Context, Result};
use reqwest::blocking::{multipart, Client};
use serde_json::Value;

pub fn upload_image(client: &Client, path: &Path) -> Result<String> {
    let mut errors = Vec::new();

    match upload_litterbox(client, path) {
        Ok(url) => return Ok(url),
        Err(err) => errors.push(format!("Litterbox: {err}")),
    }

    match upload_tmpfiles(client, path) {
        Ok(url) => return Ok(url),
        Err(err) => errors.push(format!("Tmpfiles: {err}")),
    }

    match upload_0x0(client, path) {
        Ok(url) => return Ok(url),
        Err(err) => errors.push(format!("0x0.st: {err}")),
    }

    match upload_fileio(client, path) {
        Ok(url) => return Ok(url),
        Err(err) => errors.push(format!("File.io: {err}")),
    }

    bail!("không tải được screenshot lên dịch vụ tạm: {}", errors.join("; "))
}

fn upload_litterbox(client: &Client, path: &Path) -> Result<String> {
    let body = post_multipart(
        client,
        "https://litterbox.catbox.moe/resources/internals/api.php",
        "fileToUpload",
        path,
        &[("reqtype", "fileupload"), ("time", "1h")],
    )?;
    parse_plain_url(&body)
}

fn upload_tmpfiles(client: &Client, path: &Path) -> Result<String> {
    let body = post_multipart(
        client,
        "https://tmpfiles.org/api/v1/upload",
        "file",
        path,
        &[],
    )?;
    let value: Value = serde_json::from_str(&body).context("Tmpfiles trả JSON không hợp lệ")?;
    let raw = value
        .pointer("/data/url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Tmpfiles không trả URL"))?;

    let trimmed = raw
        .trim_start_matches("https://tmpfiles.org/")
        .trim_start_matches("http://tmpfiles.org/")
        .trim_start_matches("dl/");
    Ok(format!("https://tmpfiles.org/dl/{trimmed}"))
}

fn upload_0x0(client: &Client, path: &Path) -> Result<String> {
    let body = post_multipart(client, "https://0x0.st", "file", path, &[])?;
    parse_plain_url(&body)
}

fn upload_fileio(client: &Client, path: &Path) -> Result<String> {
    let body = post_multipart(client, "https://file.io", "file", path, &[])?;
    let value: Value = serde_json::from_str(&body).context("File.io trả JSON không hợp lệ")?;
    value
        .get("link")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("File.io không trả link"))
}

fn post_multipart(
    client: &Client,
    endpoint: &str,
    file_field: &str,
    path: &Path,
    fields: &[(&str, &str)],
) -> Result<String> {
    let mut form = multipart::Form::new();
    for (name, value) in fields {
        form = form.text((*name).to_string(), (*value).to_string());
    }
    form = form
        .file(file_field.to_string(), path)
        .with_context(|| format!("không mở được {} để upload", path.display()))?;

    let response = client
        .post(endpoint)
        .timeout(Duration::from_secs(120))
        .multipart(form)
        .send()
        .context("lỗi kết nối dịch vụ upload tạm")?;

    let status = response.status();
    let body = response.text().context("không đọc được phản hồi upload")?;
    if !status.is_success() {
        bail!("HTTP {}", status.as_u16());
    }
    Ok(body)
}

fn parse_plain_url(body: &str) -> Result<String> {
    let value = body.trim();
    if value.starts_with("https://") || value.starts_with("http://") {
        return Ok(value.to_string());
    }
    bail!("dịch vụ không trả URL hợp lệ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_https_url() {
        assert_eq!(
            parse_plain_url(" https://example.com/a.png\n").unwrap(),
            "https://example.com/a.png"
        );
    }
}
