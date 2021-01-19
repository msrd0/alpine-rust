FROM alpine:{{ alpine }}

COPY {{ pubkey }} /etc/apk/keys/
COPY simple_compiler_test.tar /opt/simple_compiler_test.tar
RUN sed -i 's,http:,https:,g' /etc/apk/repositories \
 && echo "http://[{{ cidr_v6.first_ip() }}]:2015" >>/etc/apk/repositories
