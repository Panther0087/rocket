#!/bin/bash
set -e

#
# Builds the rustdocs for all of the libraries.
#

# Brings in: ROOT_DIR, EXAMPLES_DIR, LIB_DIR, CODEGEN_DIR, CONTRIB_DIR, DOC_DIR
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
source $SCRIPT_DIR/config.sh

function mk_doc() {
  local dir=$1
  local flag=$2
  pushd $dir > /dev/null
    echo ":: Documenting '${dir}'..."
    cargo doc --no-deps --all-features
    cargo doc --no-deps $flag
  popd > /dev/null
}

# We need to clean-up beforehand so we don't get all of the dependencies.
cargo clean

mk_doc $LIB_DIR
mk_doc $CODEGEN_DIR
mk_doc $CONTRIB_DIR  --all-features

# Blank index, for redirection.
touch ${DOC_DIR}/index.html
