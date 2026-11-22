// Copyright 2016 Google Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

package dhcp4

import (
	"testing"
)

func TestOptionByte(t *testing.T) {
	o := Options{
		1: []byte{3},
		2: []byte{1, 2, 3},
	}

	b, err := o.Byte(1)
	if err != nil {
		t.Fatal(err)
	}
	if b != 3 {
		t.Fatalf("wanted value 3, got %d", b)
	}

	b, err = o.Byte(2)
	if err == nil {
		t.Fatalf("option shouldn't be a valid byte")
	}
}

func TestOptionUint16(t *testing.T) {
	o := Options{
		1: []byte{1, 2},
		2: []byte{1, 2, 3},
	}

	u, err := o.Uint16(1)
	if err != nil {
		t.Fatal(err)
	}
	if u != 258 {
		t.Fatalf("wanted value 258, got %d", u)
	}

	u, err = o.Uint16(2)
	if err == nil {
		t.Fatal("option shouldn't be a valid uint16")
	}
}
