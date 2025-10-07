.POSIX:
.PHONY: default build dev test fmt

default: build

build:
	nix build .

dev:
	nix run . -- \
		--installer ./examples#nixosConfigurations.installer \
		--flake ./examples \
		--hosts ./examples/hosts.json \
		--ssh-key ~/.ssh/id_ed25519 \
		--debug

test:
	go test -v ./...

fmt:
	go fmt ./...
	treefmt
