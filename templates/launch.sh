#!/bin/bash
set -e

# install docker-ce and stop it again after the stupid autostart
sudo apt-get update -y
sudo apt-get install -y --no-install-recommends apt-transport-https ca-certificates curl gnupg-agent software-properties-common
curl -fsSL https://download.docker.com/linux/debian/gpg | sudo apt-key add -
sudo add-apt-repository 'deb [arch=amd64] https://download.docker.com/linux/debian buster stable'
sudo apt-get update -y
sudo apt-get install -y --no-install-recommends docker-ce
sudo systemctl stop docker
sudo systemctl disable docker

# upload docker certificates
sudo mkdir /etc/docker-certs
sudo chmod 777 /etc/docker-certs
cat >/etc/docker-certs/ca.pem <<EOF
{{ ca_pem }}
EOF
cat >/etc/docker-certs/cert.pem <<EOF
{{ cert_pem }}
EOF
cat >/etc/docker-certs/key.pem <<EOF
{{ key_pem }}
EOF
sudo chown root /etc/docker-certs/*
sudo chmod 400 /etc/docker-certs/*

# install custom docker systemd unit
sudo chmod 777 /etc/systemd/system
cat >/etc/systemd/system/docker-tlsverify.service <<EOF
# Adopted from /lib/systemd/system/docker.service
[Unit]
Description=Docker Application Container Engine
Documentation=https://docs.docker.com
BindsTo=containerd.service
After=network-online.target firewalld.service containerd.service
Wants=network-online.target

[Service]
Type=notify
# the default is not to use systemd for cgroups because the delegate issues still
# exists and systemd currently does not support the cgroup feature set required
# for containers run by docker
ExecStart=/usr/bin/dockerd --tlsverify --tlscacert=/etc/docker-certs/ca.pem --tlscert=/etc/docker-certs/cert.pem --tlskey=/etc/docker-certs/key.pem -H=0.0.0.0:2376 --containerd=/run/containerd/containerd.sock
ExecReload=/bin/kill -s HUP \$MAINPID
TimeoutSec=0
RestartSec=2
Restart=always

# Note that StartLimit* options were moved from "Service" to "Unit" in systemd 229.
# Both the old, and new location are accepted by systemd 229 and up, so using the old location
# to make them work for either version of systemd.
StartLimitBurst=3

# Note that StartLimitInterval was renamed to StartLimitIntervalSec in systemd 230.
# Both the old, and new name are accepted by systemd 230 and up, so using the old name to make
# this option work for either version of systemd.
StartLimitInterval=60s

# Having non-zero Limit*s causes performance problems due to accounting overhead
# in the kernel. We recommend using cgroups to do container-local accounting.
LimitNOFILE=infinity
LimitNPROC=infinity
LimitCORE=infinity

# Comment TasksMax if your systemd version does not support it.
# Only systemd 226 and above support this option.
TasksMax=infinity

# set delegate yes so that systemd does not reset the cgroups of docker containers
Delegate=yes

# kill only the docker process, not all processes in the cgroup
KillMode=process

[Install]
WantedBy=multi-user.target
EOF
sudo systemctl enable --now docker-tlsverify
