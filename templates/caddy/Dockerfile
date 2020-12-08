FROM caddy:2-alpine

# use custom Caddyfile
COPY Caddyfile /etc/caddy/Caddyfile

# this is where the repository will be mounted
VOLUME /repo

# caddy will run on port 2015
EXPOSE 2015
