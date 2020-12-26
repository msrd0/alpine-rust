FROM alpine:{{ alpine }}

{% let suffix -%}
{% match channel -%}
  {% when Some with (channel) -%}
    {% let suffix = channel.to_string() -%}
  {% when None -%}
    {% let suffix = rustver.to_string() -%}
{% endmatch -%}
COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "https://msrd0cdn.de/alpine-rust/{{ alpine }}/alpine-rust/" >>/etc/apk/repositories \
 && apk add --no-cache \
      cargo-{{ suffix }} \
      gcc \
      musl-dev \
      rust-{{ suffix }}
