.POSIX:
.PHONY: default build dev test test-e2e fmt

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

test-e2e:
	sudo env PATH="$$PATH" nix run --print-build-logs .#e2e

fmt:
	go fmt ./...
	treefmt
