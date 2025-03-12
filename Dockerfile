# syntax=docker/dockerfile
FROM rust:1.85-alpine3.21 AS builder

WORKDIR /project
COPY . /project/

# hadolint ignore=DL3018
RUN --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=private,target=/project/target \
    apk --no-cache add build-base musl-dev && \
    cargo build --release && \
    cp -v target/release/ruby-version-checker /ruby-version-checker

FROM scratch
COPY --from=builder /ruby-version-checker /ruby-version-checker

LABEL maintainer="Moritz Heiber <hello@heiber.im>"
LABEL org.opencontainers.image.source=https://github.com/moritzheiber/ruby-version-checker-rs

ENV RUST_LOG="info"

CMD ["/ruby-version-checker"]
