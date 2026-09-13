// Package main wraps the kepubify library in one exported C function so a
// Rust build script can link it as a static archive.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"archive/zip"
	"context"
	"os"

	"github.com/pgaskin/kepubify/v4/kepub"
)

// KepubConvert converts the EPUB at in into a KEPUB at out. It returns nil on
// success and a C string with the error text on failure. The caller frees the
// string with free.
//
//export KepubConvert
func KepubConvert(in *C.char, out *C.char) *C.char {
	if err := convert(C.GoString(in), C.GoString(out)); err != nil {
		return C.CString(err.Error())
	}
	return nil
}

func convert(in, out string) error {
	zr, err := zip.OpenReader(in)
	if err != nil {
		return err
	}
	defer zr.Close()
	f, err := os.Create(out)
	if err != nil {
		return err
	}
	if err := kepub.NewConverter().Convert(context.Background(), f, zr); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}

func main() {}
