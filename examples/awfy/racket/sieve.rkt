#lang racket/base
;; Sieve over a Racket treelist (immutable RRB tree, functional `treelist-set`)
;; — the direct analog of Twinkle's persistent Vector<Bool>.
(require racket/treelist "harness.rkt")
(provide sieve-bench)

(define (sieve-mark flags size i)
  (let loop ([k (* 2 i)] [f flags])
    (if (> k size) f (loop (+ k i) (treelist-set f k #f)))))

(define (sieve-run size)
  (define init
    (let loop ([i 0] [t (treelist)])
      (if (<= i size) (loop (add1 i) (treelist-add t #t)) t)))
  (let loop ([i 2] [flags init] [count 0])
    (cond
      [(> i size) count]
      [(treelist-ref flags i) (loop (add1 i) (sieve-mark flags size i) (add1 count))]
      [else (loop (add1 i) flags count)])))

(define sieve-bench (bench "sieve" 10 40 5000 669 sieve-run))
