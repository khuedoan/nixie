package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math/rand"
	"net/http"
	"time"

	"code.khuedoan.com/nixie/internal/serve"
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

func install(address string, installRequest serve.InstallRequest) error {
	client := &http.Client{Timeout: 5 * time.Second}
	body, err := json.Marshal(&installRequest)
	if err != nil {
		return err
	}

	resp, err := client.Post(
		fmt.Sprintf("http://%s/install", address),
		"application/json",
		bytes.NewBuffer(body),
	)

	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusAccepted {
		return fmt.Errorf("install request failed: %s", resp.Status)
	}

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		log.Printf("failed to read response body: %v", err)
	} else {
		log.Printf("successfully requested installation: %s", string(respBody))
	}

	return err
}
