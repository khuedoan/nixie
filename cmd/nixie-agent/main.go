package main

import (
	"log"

	"code.khuedoan.com/nixie/internal/serve"
)

func main() {
	params, err := getAgentConfig()
	if err != nil {
		log.Fatalf("failed to get Nixie params: %v", err)
	}
	log.Printf("nixie-agent params: %+v", params)

	if err = ping(params.APIAddress); err != nil {
		log.Fatalf("failed to ping Nixie API server: %v", err)
	}
	log.Printf("successfully sent ping to API server")

	installRequest := serve.InstallRequest{
		MACAddress: params.MACAddress,
	}
	if err = install(params.APIAddress, installRequest); err != nil {
		log.Fatalf("failed to request for installation: %v", err)
	}
}
