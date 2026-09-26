#!/usr/bin/env bash
# Builds the player guide: a book per language from this directory, English from src/en into
# <dest>/en and Russian from src/ru into <dest>/ru. oracle-web serves them under /guide/en/ and
# /guide/ru/, and src/images, which both books reference as ../images/<lang>/, under /guide/images/.
#
#     docs/guide/build.sh <absolute dest dir>
#
# MDBOOK names the mdbook binary, `mdbook` on PATH by default. The Dockerfile's guide stage and
# lanes/dev.sh run it with the release the Dockerfile pins.
set -euo pipefail

# mdBook 0.5 resolves a relative --dest-dir against the working directory, not the book.
if [[ $# -ne 1 || $1 != /* ]]; then
  echo "usage: $0 <absolute dest dir>" >&2
  exit 2
fi
dest=$1
book=$(dirname "$0")

# book.toml holds what the books share; these differ. site-url is where the book is published: the
# 404 page, which the server sends for any missing page in the book, resolves its links from there.
build() { # <lang> <description>
  MDBOOK_BOOK__SRC=src/$1 MDBOOK_BOOK__LANGUAGE=$1 MDBOOK_BOOK__DESCRIPTION=$2 \
    MDBOOK_OUTPUT__HTML__SITE_URL=/guide/$1/ \
    "${MDBOOK:-mdbook}" build "$book" --dest-dir "$dest/$1"
}
build en "Player guide for PoE2 Oracle, a price-check overlay for Path of Exile 2 on Windows."
build ru "Руководство по PoE2 Oracle — оверлею для проверки цен в Path of Exile 2 под Windows."
