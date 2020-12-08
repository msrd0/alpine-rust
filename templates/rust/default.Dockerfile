FROM alpine:{{ alpine }}

{% let suffix -%}
{% match rustver -%}
  {% when Some with (ver) -%}
    {% let suffix = format!("-{}", ver) -%}
  {% when None -%}
    {% let suffix = String::new() -%}
{% endmatch -%}
COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "https://msrd0cdn.de/alpine-rust/{{ alpine }}/alpine-rust/" >>/etc/apk/repositories \
 && apk add --no-cache \
      cargo{{ suffix }} \
      clang \
      clippy{{ suffix }} \
      gcc \
      lld \
      musl-dev \
      rust{{ suffix }} \
      rustfmt{{ suffix }}

ENV CC=/usr/bin/clang
ENV CXX=/usr/bin/clang++
ENV RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=lld"
