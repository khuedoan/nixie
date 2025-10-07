package main

import (
	"errors"
	"os"
	"strings"
)

type NixieParams struct {
	MAC string
	API string
}

func GetNixieParams() (*NixieParams, error) {
	data, err := os.ReadFile("/proc/cmdline")
	if err != nil {
		return nil, err
	}

	params := parseKernelParams(string(data))
	nixieParams := &NixieParams{
		MAC: params["nixie_mac_address"],
		API: params["nixie_api"],
	}
	if nixieParams.MAC == "" || nixieParams.API == "" {
		return nil, errors.New("missing required kernel parameters: nixie_mac_address or nixie_api")
	}

	return nixieParams, nil
}

func parseKernelParams(cmdline string) map[string]string {
	params := make(map[string]string)

	for field := range strings.FieldsSeq(cmdline) {
		parts := strings.SplitN(field, "=", 2)
		if len(parts) != 2 {
			continue
		}
		params[parts[0]] = parts[1]
	}

	return params
}
