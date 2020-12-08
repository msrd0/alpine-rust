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
      gcc \
      musl-dev \
      rust{{ suffix }}
