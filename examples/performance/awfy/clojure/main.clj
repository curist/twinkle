;; Runs the persistent-structure subset (Sieve, Bounce, NBody) in Clojure, using
;; ordinary persistent vectors — the same broad representation family as
;; Twinkle's Vector<T>. Loaded via load-file (paths relative to the repo root,
;; where run.sh invokes this).
(load-file "examples/performance/awfy/clojure/harness.clj")
(load-file "examples/performance/awfy/clojure/sieve.clj")
(load-file "examples/performance/awfy/clojure/bounce.clj")
(load-file "examples/performance/awfy/clojure/nbody.clj")

(run-all [sieve-bench bounce-bench nbody-bench
          sieve-mut-bench bounce-mut-bench nbody-mut-bench])
