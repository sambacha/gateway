#!/bin/bash
sudo apt update
curl -fsSL https://apt.releases.hashicorp.com/gpg | sudo apt-key add -
sudo apt-add-repository "deb [arch=$(dpkg --print-architecture)] https://apt.releases.hashicorp.com $(lsb_release -cs) main"
sudo apt install terraform
pip3 install awscli
aws configure
cd ops/

echo "Terraform starting..."

terraform init -upgrade \
  -backend-config="bucket=compound-gateway" \
  -backend-config="key=tfstate" \
  -backend-config="region=ap-southeast-2"
