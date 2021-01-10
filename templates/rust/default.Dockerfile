FROM alpine:{{ alpine }}

COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "https://msrd0cdn.de/alpine-rust/{{ alpine }}/alpine-rust/" >>/etc/apk/repositories \
 && apk add --no-cache \
      cargo-{{ channel }} \
      clang \
      clippy-{{ channel }} \
      gcc \
      lld \
      musl-dev \
      rust-{{ channel }} \
      rustfmt-{{ channel }}

ENV CC=/usr/bin/clang
ENV CXX=/usr/bin/clang++
ENV RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=lld"
