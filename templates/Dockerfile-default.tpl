FROM alpine:{{ alpine }}

COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "https://media.githubusercontent.com/media/msrd0/alpine-rust/gh-pages/{{ alpine }}/alpine-rust" >>/etc/apk/repositories \
 && apk add --no-cache \
      cargo-{{ rustver }} \
      clang \
      gcc \
      lld \
      musl-dev \
      rust-{{ rustver }}

ENV CC=/usr/bin/clang
ENV CXX=/usr/bin/clang++
ENV RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=lld"
