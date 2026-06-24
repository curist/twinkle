#lang racket/base
;; Racket treelist mirror of boot/bench/append_push.tw
;; Single-element push at the end via treelist-add. This is the case where
;; Twinkle's tail buffer (amortized O(1) push) has no counterpart in a pure
;; RRB tree, which must descend the right spine on every push.
;;   racket boot/bench/racket/append_push.rkt
(require racket/treelist)

(define (bench n)
  (define t0 (current-inexact-milliseconds))
  (let loop ([i 0] [acc (treelist)])
    (cond
      [(= i n)
       (define dt (- (current-inexact-milliseconds) t0))
       (printf "~a\t~a\t~a\n" n dt (treelist-ref acc (sub1 (treelist-length acc))))]
      [else
       (loop (add1 i) (treelist-add acc i))])))

(printf "N\tms\tsink\n")
(for ([n (in-list '(1000 2000 4000 8000 16000 32000))])
  (bench n))
