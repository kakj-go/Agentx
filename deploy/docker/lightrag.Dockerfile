FROM ghcr.io/hkuds/lightrag@sha256:206579ab1a2bc1f2b5f19fe97c01cb801ff409e6ae29a15542f82208a285640a

ENV TIKTOKEN_CACHE_DIR=/opt/tiktoken-cache

RUN mkdir -p "$TIKTOKEN_CACHE_DIR" \
    && python -c "import tiktoken; tiktoken.encoding_for_model('gpt-4o-mini')" \
    && chmod -R a+rX "$TIKTOKEN_CACHE_DIR"
