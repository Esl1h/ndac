# Fedora COPR "make srpm" method: builds the source RPM of the latest
# release. COPR runs `make -f .copr/Makefile srpm outdir=... spec=...` in a
# fresh checkout, as root, with network access. packaging.yml starts it
# through a COPR custom webhook once a GitHub release is published.

outdir ?= $(CURDIR)
repo := https://github.com/Esl1h/ndac
version := $(shell git describe --tags --abbrev=0 --match 'v[0-9]*' | sed 's/^v//')
targets := x86_64-unknown-linux-musl aarch64-unknown-linux-musl

srpm:
	dnf -y install rpm-build curl
	mkdir -p "$(outdir)/sources"
	curl -fsSL -o "$(outdir)/sources/SHA256SUMS" "$(repo)/releases/download/v$(version)/SHA256SUMS"
	for t in $(targets); do \
	  curl -fsSL -o "$(outdir)/sources/ndac-$(version)-$$t.tar.gz" \
	    "$(repo)/releases/download/v$(version)/ndac-$(version)-$$t.tar.gz" || exit 1; \
	done
	cd "$(outdir)/sources" && sha256sum -c --ignore-missing --strict SHA256SUMS
	sed 's/^Version:.*/Version:        $(version)/' packaging/rpm/ndac.spec >"$(outdir)/ndac.spec"
	rpmbuild -bs --define "_sourcedir $(outdir)/sources" --define "_srcrpmdir $(outdir)" "$(outdir)/ndac.spec"

.PHONY: srpm
