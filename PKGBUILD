# Maintainer: Lam <lam@example.com>
pkgname=linguist-anki-bridge
pkgver=0.1.0
pkgrel=1
pkgdesc="Automate and enhance language learning workflows with Anki and LLMs"
arch=('any')
url="https://github.com/lam/linguist-anki-bridge"
license=('MIT')
depends=('python-click' 'python-rich' 'python-requests' 'python-beautifulsoup4' 'python-google-genai' 'python-pydantic-settings')
makedepends=('python-build' 'python-installer' 'python-wheel' 'python-hatchling')
source=("${pkgname}-${pkgver}.tar.gz::https://github.com/lam/${pkgname}/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$pkgname-$pkgver"
  python -m build --wheel --no-isolation
}

package() {
  cd "$pkgname-$pkgver"
  python -m installer --destdir="$pkgdir" dist/*.whl
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
