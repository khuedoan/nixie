.POSIX:
.PHONY: default build dev test fmt

default: build

build:
	nix build .

dev:
	sudo nix run . -- \
		--installer ./examples#nixosConfigurations.installer \
		--flake ./examples \
		--hosts ./examples/hosts.json \
		--install-ssh-key ~/.ssh/nixie-install \
		--deployment-ssh-key ~/.ssh/nixie-deployment \
		--debug

test:
	go test -v ./...

fmt:
	go fmt ./...
	treefmt
