FROM alpine:{{ alpine }}

COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "https://media.githubusercontent.com/media/msrd0/alpine-rust/gh-pages/{{ alpine }}/alpine-rust" >>/etc/apk/repositories \
 && apk add --no-cache \
      cargo-{{ rustver }} \
      gcc \
      musl-dev \
      rust-{{ rustver }}
