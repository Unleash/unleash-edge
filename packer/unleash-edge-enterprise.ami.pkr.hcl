packer {
  required_version = ">= 1.11.0"
  required_plugins {
    amazon = {
      source  = "github.com/hashicorp/amazon"
      version = ">= 1.2.0"
    }
  }

}
variable "edge_version" {
  type        = string
  description = "Tag to build "
}


variable "build_region" {
  type        = string
  description = "AWS region to build the AMI in"
}

variable "replicate_regions" {
  type = list(string)
  description = "AWS regions to replicate the AMI to"
}

variable "instance_type" {
  type        = string
  description = "Instance type used for building the AMI"
}

variable "ssh_username" {
  type        = string
  description = "SSH username for the instance"
}

source "amazon-ebs" "unleash-edge-enterprise" {
  ami_name                    = "unleash-edge-enterprise-arm64-ami-{{timestamp}}"
  instance_type               = var.instance_type
  region                      = var.build_region
  ssh_username                = var.ssh_username
  ami_virtualization_type     = "hvm"
  associate_public_ip_address = true
  ami_regions                 = var.replicate_regions

  source_ami_filter {
    most_recent = true
    owners = ["099720109477"]
    filters = {
      name                = "ubuntu-minimal/images/hvm-ssd-gp3/ubuntu-noble-24.04-arm64-minimal-*"
      architecture        = "arm64"
      root-device-type    = "ebs"
      virtualization-type = "hvm"
      state               = "available"
    }
  }

  tags = {
    Name        = "Unleash Edge Enterprise ARM64 AMI"
    EdgeVersion = var.edge_version
  }
}

build {
  name = "build-edge-enterprise-arm64"
  sources = ["source.amazon-ebs.unleash-edge-enterprise"]
  provisioner "shell" {
    script            = "${path.root}/provisioners/00-base.sh"
    execute_command   = "sudo -E bash '{{.Path}}'"
    expect_disconnect = true
    valid_exit_codes = [0]
  }

  provisioner "shell" {
    script = "${path.root}/provisioners/05-rust.sh"
    valid_exit_codes = [0]
  }

  provisioner "shell" {
    script = "${path.root}/provisioners/10-clone-and-build.sh"
    environment_vars = ["EDGE_VERSION=${var.edge_version}"]
    valid_exit_codes = [0]
  }

  provisioner "shell" {
    script            = "${path.root}/provisioners/15-hardening.sh"
    execute_command   = "sudo -E bash '{{.Path}}'"
    expect_disconnect = true
    valid_exit_codes = [0]
  }

  provisioner "shell" {
    script          = "${path.root}/provisioners/20-unleash-edge-service-unit.sh"
    execute_command = "sudo -E bash '{{.Path}}'"
    valid_exit_codes = [0]
  }
  provisioner "shell" {
    script = "${path.root}/provisioners/25-clean-build-tools.sh"
    valid_exit_codes = [0]
  }
  provisioner "shell" {
    inline = ["echo ✅ Build complete: unleash-edge-enterprise-arm64-ubuntu-24.04"]
  }
  post-processor "manifest" {
    output = "${path.root}/unleash-edge-enterprise-arm64-ami-manifest.json"
  }

}
