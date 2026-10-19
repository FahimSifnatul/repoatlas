class Repoatlas < Formula
  desc "Turn any code repository into an interactive, layer-by-layer architecture map"
  homepage "https://github.com/FahimSifnatul/repoatlas"
  url "https://github.com/FahimSifnatul/repoatlas/archive/refs/tags/v1.0.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_THE_TARBALL"
  license "MIT" # use the licence you actually put in the repo
  head "https://github.com/FahimSifnatul/repoatlas.git", branch: "master"

  # repoatlas only uses Python's standard library, so there are no `resource` blocks to maintain.
  depends_on "python@3.13"

  def install
    # Keep the package in libexec and put a tiny launcher on the PATH that runs it with Homebrew's Python.
    libexec.install "repoatlas"
    (bin/"repoatlas").write <<~SH
      #!/bin/bash
      PYTHONPATH="#{libexec}${PYTHONPATH:+:$PYTHONPATH}" exec "#{Formula["python@3.13"].opt_bin}/python3.13" -m repoatlas "$@"
    SH
    chmod 0755, bin/"repoatlas"
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
