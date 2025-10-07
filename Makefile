.POSIX:
.PHONY: default build dev test fmt

default: build

build:
	nix build

dev:
	nix run

test:
	nix flake check

fmt:
	go fmt ./...
	treefmt
