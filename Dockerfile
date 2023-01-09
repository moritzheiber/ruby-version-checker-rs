# syntax=docker/dockerfile
FROM rust:1.66-alpine3.17 as builder

WORKDIR /project
COPY . /project/

# hadolint ignore=DL3018
RUN --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=private,target=/build/target \
    apk --no-cache add build-base musl-dev && \
    cargo build --release

FROM scratch
COPY --from=builder /project/target/release/ruby-version-checker /ruby-version-checker

LABEL maintainer="Moritz Heiber <hello@heiber.im>"
LABEL org.opencontainers.image.source=https://github.com/moritzheiber/ruby-version-checker-rs

ENV RUST_LOG="info"

CMD ["/ruby-version-checker"]
