#lang racket/base
;; Timing + checksum harness shared by the Racket benchmark modules.
(provide (struct-out bench) run-bench run-all)

(struct bench (name warmup iters size expected run))

(define (run-bench b)
  (for ([_ (in-range (bench-warmup b))]) ((bench-run b) (bench-size b)))
  (define start (current-inexact-milliseconds))
  (define checksum
    (let loop ([i 0] [c 0])
      (if (< i (bench-iters b))
          (loop (add1 i) ((bench-run b) (bench-size b)))
          c)))
  (define elapsed (- (current-inexact-milliseconds) start))
  (unless (= checksum (bench-expected b))
    (error (format "~a: checksum ~a != expected ~a" (bench-name b) checksum (bench-expected b))))
  (printf "racket\t~a\t~a\t~a\t~a\n" (bench-name b) (bench-iters b) elapsed checksum))

(define (run-all bs)
  (for ([b (in-list bs)]) (run-bench b)))
