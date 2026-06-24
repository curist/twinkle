#lang racket/base
;; Racket treelist mirror of boot/bench/concat_prepend.tw
;; Right-operand accumulator concat (prepend): (treelist-append (treelist i) acc).
;; RRB should make this near-linear; a tail-only/radix vector is O(n^2).
;;   racket boot/bench/racket/concat_prepend.rkt
(require racket/treelist)

(define (bench n)
  (define t0 (current-inexact-milliseconds))
  (let loop ([i 0] [acc (treelist)])
    (cond
      [(= i n)
       (define dt (- (current-inexact-milliseconds) t0))
       ;; acc[0] is the most-recently prepended element (n-1): cheap sink.
       (printf "~a\t~a\t~a\n" n dt (treelist-ref acc 0))]
      [else
       (loop (add1 i) (treelist-append (treelist i) acc))])))

(printf "N\tms\tsink\n")
(for ([n (in-list '(1000 2000 4000 8000 16000))])
  (bench n))
