FROM scratch
COPY stormd /stormd
COPY cloudid /cloudid
COPY deploy/stormd-config.toml /etc/stormd/config.toml
# Directory skeleton fallback (log dir lives on the cloudid-data PVC in
# normal operation; this keeps log-open from erroring if the PVC is absent)
COPY deploy/rootfs/ /
EXPOSE 8090 9080 22
ENTRYPOINT ["/stormd"]
CMD ["--config", "/etc/stormd/config.toml"]
