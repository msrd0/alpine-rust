FROM alpine:{{ alpine }}

COPY {{ pubkey }} /etc/apk/keys/
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "http://[{{ cidr_v6.first_ip() }}]:2015" >>/etc/apk/repositories
