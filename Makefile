VERSION := $(shell git describe --tags --always 2>/dev/null || echo "dev")
LDFLAGS := -s -w -X main.version=$(VERSION)
BINARY := slqm

.PHONY: all build clean test lint

all: build

build:
	CGO_ENABLED=0 go build -ldflags="$(LDFLAGS)" -o dist/$(BINARY) ./cmd/slqm

# Cross-compilation targets
build-arm64:
	GOOS=linux GOARCH=arm64 CGO_ENABLED=0 \
		go build -ldflags="$(LDFLAGS)" -o dist/$(BINARY)-arm64 ./cmd/slqm

build-armv7:
	GOOS=linux GOARCH=arm GOARM=7 CGO_ENABLED=0 \
		go build -ldflags="$(LDFLAGS)" -o dist/$(BINARY)-armv7 ./cmd/slqm

build-mipsle:
	GOOS=linux GOARCH=mipsle GOMIPS=softfloat CGO_ENABLED=0 \
		go build -ldflags="$(LDFLAGS)" -o dist/$(BINARY)-mipsle ./cmd/slqm

build-all: build-arm64 build-armv7 build-mipsle

test:
	go test -v -race ./...

test-cover:
	go test -coverprofile=coverage.out ./...
	go tool cover -html=coverage.out -o coverage.html

lint:
	go vet ./...

clean:
	rm -rf dist/ coverage.out coverage.html

# Package for opkg
package: build-all
	./scripts/package.sh
