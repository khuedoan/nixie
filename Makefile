.POSIX:
.PHONY: default build fmt

default: build

build:
	nix build .

fmt:
	go fmt ./...
	treefmt
