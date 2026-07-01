#lang racket/base
;; Runs the persistent-structure subset (Sieve, Bounce, NBody) in Racket, using
;; treelists — the same broad representation family as Twinkle's Vector<T>.
(require "harness.rkt" "sieve.rkt" "bounce.rkt" "nbody.rkt")

(run-all (list sieve-bench bounce-bench nbody-bench))
