package main

import (
	"reflect"
	"testing"
)

func TestEncodedBootValues(t *testing.T) {
	got, err := parseKernelParams("init=/nix/store/installer+source/init " +
		"nixie_mac_address=02%3A00%3A00%3A00%3A00%3A01 " +
		"nixie_api=%5Bfd00%3A%3A1%5D%3A5000 " +
		"nixie_flake=http%3A%2F%2Fcontroller%2Fsource.tar.gz%3Fdir%3Dexample%2520dir%26rev%3Da%2Bb%23node+with+spaces")
	if err != nil {
		t.Fatal(err)
	}

	want := map[string]string{
		"init":              "/nix/store/installer+source/init",
		"nixie_mac_address": "02:00:00:00:00:01",
		"nixie_api":         "[fd00::1]:5000",
		"nixie_flake":       "http://controller/source.tar.gz?dir=example%20dir&rev=a+b#node with spaces",
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("decoded boot values = %+v, want %+v", got, want)
	}
}
