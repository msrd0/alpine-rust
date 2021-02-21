FROM alpine:{{ alpine }}

LABEL org.opencontainers.image.url="https://github.com/users/msrd0/packages/container/package/alpine-rust"
LABEL org.opencontainers.image.title="alpine-rust minimal (rustc {{ channel }})"
LABEL org.opencontainers.image.description="Alpine Linux based Docker Image with minimal Rust {{ channel }} pre-installed"
LABEL org.opencontainers.image.source="https://github.com/msrd0/alpine-rust"
LABEL org.opencontainers.image.revision="{{ git_commit }}"

COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "https://msrd0cdn.de/alpine-rust/{{ alpine }}/alpine-rust/" >>/etc/apk/repositories \
 && apk add --no-cache \
      cargo-{{ channel }} \
      gcc \
      musl-dev \
      rust-{{ channel }}
