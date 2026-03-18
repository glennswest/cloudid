FROM scratch
COPY stormd /stormd
COPY cloudid /cloudid
COPY deploy/stormd-config.toml /etc/stormd/config.toml
EXPOSE 8090 9080 22
ENTRYPOINT ["/stormd"]
CMD ["--config", "/etc/stormd/config.toml"]
