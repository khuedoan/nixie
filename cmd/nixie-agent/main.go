package main

import (
	"log"
)

func main() {
	params, err := getNixieParams()
	if err != nil {
		log.Fatalf("failed to get Nixie params: %v", err)
	}
	log.Printf("nixie-agent params: %+v", params)

	if err = ping(params.API); err != nil {
		log.Fatalf("failed to ping Nixie API server: %v", err)
	}
}
