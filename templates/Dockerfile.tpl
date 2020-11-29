FROM alpine:{{ alpine }}

# install basic dependencies
RUN apk add --no-cache alpine-sdk sudo

# we will store the repository here
VOLUME /repo

# install our repo
RUN echo /repo/alpine/{{ alpine }}/rust >>/etc/apk/repositories
COPY {{ pubkey }} /etc/apk/keys/

# create build user
RUN adduser alpine-rust \
 	&& addgroup alpine-rust abuild \
	&& echo "alpine-rust ALL=(ALL) NOPASSWD: ALL" >/etc/sudoers \
	&& mkdir -p /var/cache/distfiles \
	&& chgrp abuild /var/cache/distfiles \
	&& chmod 775 /var/cache/distfiles
USER alpine-rust
WORKDIR /home/alpine-rust
RUN mkdir -p .abuild
COPY {{ privkey }} .abuild/
RUN echo "PACKAGER_PRIVKEY=\"/home/alpine-rust/.abuild/{{ privkey }}\"" >.abuild/abuild.conf
