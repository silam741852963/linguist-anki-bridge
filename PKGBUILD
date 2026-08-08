# Maintainer: Lam <lam@example.com>
pkgname=linguist-anki-bridge-git
_pkgname=linguist-anki-bridge
pkgver=0.0.1
pkgrel=1
pkgdesc="Bridge local Anki and Ollama with Crawl4AI scrapers for language learning"
arch=('any')
url="https://github.com/silam741852963/linguist-anki-bridge"
license=('MIT')
depends=(
    'python>=3.11'
    'python-requests'
    'python-textual'
    'python-pytesseract'
    'python-gtts'
    'python-crawl4ai'
    'python-beautifulsoup4'
    'python-pyyaml'
    'python-pillow'
    'tesseract'
)
optdepends=(
    'tesseract-data-jpn: for Japanese OCR'
    'tesseract-data-vie: for Vietnamese OCR'
    'tesseract-data-deu: for German OCR'
    'tesseract-data-chi_tra: for Traditional Chinese (Taiwanese) OCR'
)
makedepends=('git' 'python-setuptools' 'python-build' 'python-installer' 'python-wheel')
provides=("$_pkgname")
conflicts=("$_pkgname")
source=("git+$url.git")
md5sums=('SKIP')

pkgver() {
    cd "$srcdir/$_pkgname"
    git describe --long --tags | sed 's/\([^-]*-g\)/r\1/;s/-/./g' || echo "0.0.1"
}

build() {
    cd "$srcdir/$_pkgname"
    python -m build --wheel --no-isolation
}

package() {
    cd "$srcdir/$_pkgname"
    python -m installer --destdir="$pkgdir" dist/*.whl
}
