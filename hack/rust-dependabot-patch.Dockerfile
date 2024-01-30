FROM dependabot/dependabot-script
RUN rustup toolchain install 1.89 && rustup default 1.89
