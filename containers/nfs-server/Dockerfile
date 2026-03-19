FROM fedora:44
RUN dnf install -y nfs-utils && dnf clean all
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
EXPOSE 2049
ENTRYPOINT ["/entrypoint.sh"]
