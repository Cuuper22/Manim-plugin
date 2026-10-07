.PHONY: build build-engine build-runtime build-workbench dev install \
	check check-scripts check-engine check-runtime check-workbench clean

# The checks need an interpreter with the runtime's `full` and `test` extras.
PYTHON ?= python3

build: build-workbench build-engine build-runtime

build-engine:
	cargo build --release --locked -p manim-director-cli --bin manim-director

build-runtime:
	$(PYTHON) -m pip wheel --no-deps ./runtime --wheel-dir dist

build-workbench:
	npm --prefix workbench ci
	npm --prefix workbench run build

dev:
	$(PYTHON) scripts/dev.py $(PROJECT)

install:
	$(PYTHON) scripts/install.py --with-manim

check: check-scripts check-engine check-runtime check-workbench

check-scripts:
	$(PYTHON) -m ruff check scripts
	$(PYTHON) -m ruff format --check scripts
	$(PYTHON) scripts/release_integrity.py versions
	$(PYTHON) -m unittest discover -s scripts -p 'test_*.py'

check-engine:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo test --workspace --locked

check-runtime:
	$(PYTHON) -m ruff check runtime/src runtime/tests
	$(PYTHON) -m ruff format --check runtime/src runtime/tests
	$(PYTHON) -m pytest runtime/tests -rs

check-workbench:
	npm --prefix workbench test
	npm --prefix workbench run build

clean:
	cargo clean
	rm -rf dist workbench/dist runtime/build runtime/dist
