# Builds the docs.
PYTHON ?= python3
SOURCES := $(wildcard *.rst)

.PHONY: html clean
html: $(SOURCES)
	$(PYTHON) -m sphinx -b html . build/html

ifeq ($(OS),Windows_NT)
clean:
	del /q build
else
clean:
	rm -rf build
endif
