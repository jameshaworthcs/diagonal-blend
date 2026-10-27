# diagonal blend. Everything runs locally; CI runs the same on main and pull requests.

WASM_BINDGEN := 0.2.129

.PHONY: setup dev core build test test-rust test-unit test-e2e preview lighthouse fixtures icons screenshots deploy clean

setup:
	rustup target add wasm32-unknown-unknown
	@if wasm-bindgen --version 2>/dev/null | grep -q "$(WASM_BINDGEN)"; then \
		echo "wasm-bindgen $(WASM_BINDGEN) present"; \
	else \
		cargo install --locked wasm-bindgen-cli --version "=$(WASM_BINDGEN)"; \
	fi
	npm ci
	npx playwright install chromium firefox

core:
	sh scripts/build-core.sh

dev: core
	npx vite

build: core
	npm run build

test: test-rust test-unit build test-e2e

test-rust:
	cargo test --manifest-path core/Cargo.toml

test-unit:
	npx vitest run

test-e2e:
	sh scripts/with-display.sh npx playwright test

preview: build
	npm run preview

lighthouse:
	sh scripts/lighthouse.sh $(URL)

fixtures:
	python3 scripts/make-fixtures.py

icons:
	node scripts/make-icons.mjs

screenshots:
	node scripts/screenshots.mjs

# Deploys go through GitHub Pages: tag a release and push the tag.
deploy:
	@test -n "$(VERSION)" || (echo "usage: make deploy VERSION=v0.1.1" && exit 1)
	git tag -a "$(VERSION)" -m "$(VERSION)"
	git push origin "$(VERSION)"

clean:
	rm -rf dist web/src/pkg test-results playwright-report lighthouse core/target
