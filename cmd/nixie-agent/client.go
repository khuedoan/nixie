package main

import (
	"fmt"
	"log"
	"math/rand"
	"net/http"
	"time"
)

func ping(address string) error {
	const maxBackoff = time.Minute
	backoff := time.Second
	client := &http.Client{Timeout: 5 * time.Second}

	for {
		// TODO maybe add http:// to kernel params
		resp, err := client.Get(fmt.Sprintf("http://%s/ping", address))
		if resp != nil {
			resp.Body.Close()
		}

		if err != nil {
			log.Printf("failed to ping Nixie API: %v", err)
		} else {
			if resp.StatusCode == http.StatusOK {
				return nil
			}
			log.Printf("unexpected response from Nixie API: %s", resp.Status)
		}

		log.Printf("retrying in %s", backoff)
		// Sleep with random jitter to avoid thundering herd
		time.Sleep(backoff + time.Duration(rand.Int63n(int64(backoff/2))))

		backoff *= 2
		if backoff > maxBackoff {
			backoff = maxBackoff
		}
	}
}
