# ndac runtime image: the static musl binary on distroless static (CA
# certificates, a nonroot user, no shell or package manager).
#
# release.yml builds it for linux/amd64 and linux/arm64 from prebuilt
# binaries at bin/<arch>/ndac in the build context; it never compiles here.
FROM gcr.io/distroless/static-debian13:nonroot@sha256:18f4cc26c9ffa7f20a359298be7deadac5c607c8e03a02b8119f1a5fc2f82c99

ARG TARGETARCH

LABEL org.opencontainers.image.title="ndac" \
      org.opencontainers.image.description="NextDNS-as-Code: manage NextDNS profiles declaratively from TOML files" \
      org.opencontainers.image.source="https://github.com/Esl1h/ndac" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

COPY --chmod=0755 bin/${TARGETARCH}/ndac /usr/local/bin/ndac

USER nonroot:nonroot
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/ndac"]
