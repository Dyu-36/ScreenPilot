//go:build !windows

package ipc

type Lock struct{}

func Acquire(func()) (*Lock, bool, error) { return &Lock{}, true, nil }
func Signal() bool                        { return false }
func (l *Lock) Close() error              { return nil }
