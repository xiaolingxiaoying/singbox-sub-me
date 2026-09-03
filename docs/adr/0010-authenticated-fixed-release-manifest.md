# Releases require an authenticated fixed-version manifest

Installation and explicit updates must verify a publisher signature over a fixed-version release manifest before trusting its artifact URLs and SHA-256 digests. SHA-256 remains an integrity check, while signature verification supplies authenticity; failed installation or update must restore the previous managed binaries and configuration.
