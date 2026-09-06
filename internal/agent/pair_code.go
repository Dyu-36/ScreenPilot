package agent

import (
	"crypto/rand"
	"encoding/binary"
	"fmt"
)

func generatePairCode() string {
	var b [4]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "000000"
	}
	n := binary.LittleEndian.Uint32(b[:]) % 1_000_000
	return fmt.Sprintf("%06d", n)
}
