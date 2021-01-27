FROM alpine:{{ alpine }}

# install basic dependencies
RUN apk add --no-cache alpine-sdk sudo

# there's a regression in busybox 3.32 (alpine 3.13) and 3.33 (edge) that makes our pkgver invalid:
# if _date is not set, `${_date+.${_date//-/}}` should evaluate empty, but evaluates to a slash (/)
RUN echo "http://dl-cdn.alpinelinux.org/alpine/v3.12/main" >>/etc/apk/repositories \
 && apk add --no-cache busybox~=1.31 \
 && sed '$d' /etc/apk/repositories /

{% if sysver.is_some() -%}
{% let sysver = sysver.unwrap() -%}
# enable old alpine repos so we can pull older rust versions
RUN echo "http://dl-cdn.alpinelinux.org/alpine/v{{ sysver }}/community" >>/etc/apk/repositories 
{%- endif %}

# we will store the repository here
VOLUME /repo
RUN sed -i 's,REPODEST=.*,REPODEST=/repo/{{ alpine }},g' /etc/abuild.conf

# install our repo
RUN echo /repo/{{ alpine }}/alpine-rust/ >>/etc/apk/repositories
COPY {{ pubkey }} /etc/apk/keys/

# create build user
RUN adduser -D alpine-rust \
 && addgroup alpine-rust abuild \
 && echo "alpine-rust ALL=(ALL) NOPASSWD: ALL" >/etc/sudoers \
 && mkdir -p /var/cache/distfiles \
 && chgrp abuild /var/cache/distfiles \
 && chmod 775 /var/cache/distfiles
USER alpine-rust
WORKDIR /home/alpine-rust
RUN mkdir -p .abuild
COPY {{ privkey }} .abuild/
RUN echo "PACKAGER_PRIVKEY=\"/home/alpine-rust/.abuild/{{ privkey }}\"" >.abuild/abuild.conf \
 && echo "export JOBS={{ jobs }}" >>.abuild/abuild.conf \
 && echo "export MAKEFLAGS=-j{{ jobs }}" >>.abuild/abuild.conf \
 && echo "export SAMUFLAGS=-j{{ jobs }}" >>.abuild/abuild.conf

# prepare the build directory
RUN mkdir -p package
WORKDIR /home/alpine-rust/package
COPY APKBUILD ./

# the command to build is pretty straight-forward
CMD ["/bin/ash", "-c", "cat APKBUILD && sudo apk update && abuild -r"]
