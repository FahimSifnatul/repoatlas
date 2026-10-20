class Repoatlas < Formula
  desc "Turn any code repository into an interactive, layer-by-layer architecture map"
  homepage "https://github.com/FahimSifnatul/repoatlas"
  url "https://github.com/FahimSifnatul/repoatlas/archive/refs/tags/v2.0.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_THE_TARBALL"
  license "MIT" # use the licence you actually put in the repo
  head "https://github.com/FahimSifnatul/repoatlas.git", branch: "master"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/repoatlas --version")

    (testpath/"app").mkpath
    (testpath/"app/main.py").write <<~PY
      def hello():
          print("hi")
    PY
    system bin/"repoatlas", testpath/"app", "-o", testpath/"map.html", "--no-open"
    assert_match "architecture map", (testpath/"map.html").read
  end
end
