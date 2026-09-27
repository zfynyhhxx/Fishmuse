# Scanner test fixtures

Scanner integration tests create their tiny byte fixtures in temporary directories at runtime.
They do not contain copyrighted audio. The real Lofty adapter is compiled, while scanner behavior
uses a deterministic `FakeTagReader` so identity, persistence, cancellation, and read-only behavior
are tested independently from third-party codecs.
