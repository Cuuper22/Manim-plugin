.PHONY: build build-engine build-runtime build-workbench dev install check clean

PYTHON ?= python3

build: build-workbench build-engine build-runtime

build-engine:
	cargo build --release --workspace --locked

build-runtime:
	$(PYTHON) -m pip wheel --no-deps ./runtime --wheel-dir dist

build-workbench:
	npm --prefix workbench ci
	npm --prefix workbench run build

dev:
	$(PYTHON) scripts/dev.py

install:
	$(PYTHON) scripts/install.py

check:
	cargo fmt --all -- --check
	cargo test --workspace --locked
	$(PYTHON) -m pytest runtime/tests -q
	$(PYTHON) scripts/release_integrity.py versions
	$(PYTHON) -m unittest discover -s scripts -p 'test_*.py'
	npm --prefix workbench test
	npm --prefix workbench run build

clean:
	cargo clean
	rm -rf dist workbench/dist runtime/build runtime/dist
