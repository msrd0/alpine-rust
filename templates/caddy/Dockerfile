FROM caddy:2-alpine

# use custom Caddyfile
COPY Caddyfile /etc/caddy/Caddyfile

# this is where the repository will be mounted
VOLUME /repo

# caddy will run on port 2015
EXPOSE 2015

# netcat will perform health checking
HEALTHCHECK --interval=5s --timeout=2s CMD nc -z localhost 2015 || exit 1
