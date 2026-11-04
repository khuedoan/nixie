package main

import (
	"errors"
	"fmt"
	"net/url"
	"os"
	"strings"
)

type AgentConfig struct {
	MACAddress string
	APIAddress string
	Flake      string
}

func getAgentConfig() (*AgentConfig, error) {
	data, err := os.ReadFile("/proc/cmdline")
	if err != nil {
		return nil, err
	}

	params, err := parseKernelParams(string(data))
	if err != nil {
		return nil, err
	}

	nixieParams := &AgentConfig{
		MACAddress: params["nixie_mac_address"],
		APIAddress: params["nixie_api"],
		Flake:      params["nixie_flake"],
	}
	if nixieParams.MACAddress == "" || nixieParams.APIAddress == "" || nixieParams.Flake == "" {
		return nil, errors.New("missing required kernel parameters: nixie_mac_address, nixie_api, or nixie_flake")
	}

	return nixieParams, nil
}

func parseKernelParams(cmdline string) (map[string]string, error) {
	params := make(map[string]string)

	for field := range strings.FieldsSeq(cmdline) {
		parts := strings.SplitN(field, "=", 2)
		if len(parts) != 2 {
			continue
		}

		value := parts[1]
		if strings.HasPrefix(parts[0], "nixie_") {
			decoded, err := url.QueryUnescape(value)
			if err != nil {
				return nil, fmt.Errorf("decode kernel parameter %s: %w", parts[0], err)
			}

			value = decoded
		}

		params[parts[0]] = value
	}

	return params, nil
}
