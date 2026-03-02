FROM scratch
COPY cloudid /cloudid
COPY config.toml /etc/cloudid/config.toml
EXPOSE 8090
ENTRYPOINT ["/cloudid", "serve", "--config", "/etc/cloudid/config.toml"]
