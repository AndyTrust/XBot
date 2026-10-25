# Il server Google della comunità (taylorwilsdon/google_workspace_mcp, licenza MIT) da PyPI,
# versione fissata: 2.1.0 (2026-10-09). Si aggiorna solo dopo averla provata (LEGGIMI.md).
FROM python:3.12-slim

ARG WORKSPACE_MCP_VERSIONE=2.1.0
RUN pip install --no-cache-dir "workspace-mcp[disk]==${WORKSPACE_MCP_VERSIONE}" \
    && useradd --create-home --shell /usr/sbin/nologin app \
    && mkdir -p /dati && chown app:app /dati

USER app
ENV WORKSPACE_MCP_PORT=8000 \
    WORKSPACE_MCP_HOST=0.0.0.0 \
    MCP_ENABLE_OAUTH21=true \
    WORKSPACE_MCP_CREDENTIALS_DIR=/dati/credenziali \
    WORKSPACE_MCP_OAUTH_PROXY_STORAGE_BACKEND=disk \
    WORKSPACE_MCP_OAUTH_PROXY_DISK_DIRECTORY=/dati/oauth-proxy \
    WORKSPACE_ATTACHMENT_DIR=/dati/allegati \
    WORKSPACE_MCP_LOG_DIR=/dati/log \
    PYTHONUNBUFFERED=1
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD python3 -c "import urllib.request,sys; urllib.request.urlopen('http://127.0.0.1:8000/health', timeout=4)" || exit 1
ENTRYPOINT ["workspace-mcp", "--transport", "streamable-http"]
CMD ["--tools", "gmail", "drive", "calendar"]
