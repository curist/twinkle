(module
  (type $rt_types__Array (array (mut anyref)))
  (type $rt_types__String (array (mut i8)))
  (type $rt_types__VecChildren (array (mut (ref null eq))))
  (type $rt_types__VecInternal (struct (field $children (ref $rt_types__VecChildren))))
  (type $rt_types__PVec (struct (field $len i32) (field $shift i32) (field $root (ref null $rt_types__VecInternal)) (field $tail (ref $rt_types__Array))))
  (type $rt_types__HamtEntry (struct (field $hash i32) (field $key anyref) (field $val anyref)))
  (type $rt_types__HamtNode (struct (field $bitmap i32) (field $entries (ref $rt_types__Array))))
  (type $rt_types__HamtCollision (struct (field $hash i32) (field $entries (ref $rt_types__Array))))
  (type $rt_types__PDict (struct (field $size i32) (field $root (ref null $rt_types__HamtNode)) (field $order (ref $rt_types__PVec))))
  (type $rt_types__ClosureEnv (array anyref))
  (type $rt_types__ClosureFunc (func (param anyref anyref) (result anyref)))
  (type $rt_types__Closure (sub (struct (field $func_ref (ref null $rt_types__ClosureFunc)) (field $env (ref null $rt_types__ClosureEnv)))))
  (type $rt_types__Variant (struct (field $type_id i32) (field $variant_id i32) (field $payload (ref null $rt_types__Array))))
  (type $rt_types__BoxedInt (struct (field $v i64)))
  (type $rt_types__BoxedFloat (struct (field $v f64)))
  (type $rt_types__IterState (sub (struct (field $seed anyref) (field $step anyref))))
  (type $user__UserRecord_2 (struct))
  (type $user__UserRecord_3 (struct (field $f0 (mut i64)) (field $f1 (mut i64)) (field $f2 (mut i64))))
  (type $user__UserRecord_4 (struct))
  (type $user__UserRecord_5 (struct (field $f0 (mut anyref)) (field $f1 (mut (ref null $rt_types__IterState)))))
  (type $user__UserRecord_8 (struct (field $f0 (mut (ref null $rt_types__IterState))) (field $f1 (mut (ref null $rt_types__Closure)))))
  (type $user__UserRecord_9 (struct (field $f0 (mut (ref null $rt_types__IterState))) (field $f1 (mut (ref null $rt_types__Closure)))))
  (type $user__UserRecord_10 (struct (field $f0 (mut (ref null $rt_types__IterState))) (field $f1 (mut i64))))
  (type $user__UserRecord_11 (struct (field $f0 (mut i64)) (field $f1 (mut i64))))
  (type $user__option__String (struct (field $variant_id i32) (field $payload (ref null $rt_types__String))))
  (type $functype_0 (func (param f64) (result (ref $rt_types__String))))
  (type $functype_1 (func (param (ref null $rt_types__String))))
  (type $functype_2 (func (param i32) (result i32)))
  (type $functype_3 (func (param (ref $rt_types__PVec) i32) (result (ref $rt_types__Array))))
  (type $functype_4 (func (param i32 (ref eq)) (result (ref eq))))
  (type $functype_5 (func (param i32 i32 (ref null $rt_types__VecInternal) (ref eq)) (result (ref eq))))
  (type $functype_6 (func (param i32 (ref eq) i32 anyref) (result (ref eq))))
  (type $functype_7 (func (param (ref $rt_types__PVec) anyref) (result (ref $rt_types__PVec))))
  (type $functype_8 (func (param i32 anyref) (result (ref $rt_types__PVec))))
  (type $functype_9 (func (param (ref null $rt_types__PVec) i32) (result anyref)))
  (type $functype_10 (func (param (ref null $rt_types__PVec) i32 anyref) (result (ref $rt_types__PVec))))
  (type $functype_11 (func (param (ref null $rt_types__PVec)) (result i32)))
  (type $functype_12 (func (param (ref null $rt_types__PVec) (ref null $rt_types__PVec)) (result (ref $rt_types__PVec))))
  (type $functype_13 (func (param (ref null $rt_types__PVec) i32 i32) (result (ref $rt_types__PVec))))
  (type $functype_14 (func (result (ref $rt_types__Array))))
  (type $functype_15 (func (param (ref null $rt_types__PVec)) (result (ref $rt_types__Array))))
  (type $functype_16 (func (param (ref null $rt_types__Array) anyref)))
  (type $functype_17 (func (param (ref null $rt_types__Array) (ref null $rt_types__PVec))))
  (type $functype_18 (func (param (ref null $rt_types__Array)) (result (ref $rt_types__PVec))))
  (type $functype_19 (func (param (ref $rt_types__Array)) (result (ref $rt_types__PVec))))
  (type $functype_20 (func (param (ref null $rt_types__Variant)) (result (ref null $rt_types__Variant))))
  (type $functype_21 (func (param (ref null $rt_types__String)) (result i32)))
  (type $functype_22 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result (ref $rt_types__String))))
  (type $functype_23 (func (param (ref null $rt_types__String) i32 i32) (result (ref $rt_types__String))))
  (type $functype_24 (func (param (ref null $rt_types__String) (ref null $rt_types__String)) (result i32)))
  (type $functype_25 (func (param i64) (result (ref $rt_types__String))))
  (type $functype_26 (func (param i32) (result (ref $rt_types__String))))
  (type $functype_27 (func (param (ref $rt_types__Array) i32 anyref) (result (ref $rt_types__Array))))
  (type $functype_28 (func (param (ref $rt_types__Array) i32) (result (ref $rt_types__Array))))
  (type $functype_29 (func (param i64) (result i32)))
  (type $functype_30 (func (param anyref) (result i32)))
  (type $functype_31 (func (param (ref null $rt_types__HamtCollision) anyref) (result anyref)))
  (type $functype_32 (func (param (ref null $rt_types__HamtCollision) i32 anyref anyref) (result (ref $rt_types__HamtCollision))))
  (type $functype_33 (func (param (ref null $rt_types__HamtNode) i32 i32 anyref) (result anyref)))
  (type $functype_34 (func (param (ref null $rt_types__HamtNode) i32 i32 anyref anyref) (result (ref $rt_types__HamtNode))))
  (type $functype_35 (func (param (ref null $rt_types__HamtNode) i32 i32 anyref) (result (ref null $rt_types__HamtNode))))
  (type $functype_36 (func (result (ref $rt_types__PDict))))
  (type $functype_37 (func (param (ref null $rt_types__PDict)) (result i32)))
  (type $functype_38 (func (param (ref null $rt_types__PDict)) (result (ref $rt_types__PVec))))
  (type $functype_39 (func (param (ref null $rt_types__PDict) anyref) (result i32)))
  (type $functype_40 (func (param (ref null $rt_types__PDict) anyref) (result anyref)))
  (type $functype_41 (func (param (ref null $rt_types__PDict) anyref) (result (ref $rt_types__Variant))))
  (type $functype_42 (func (param (ref null $rt_types__PDict) anyref anyref) (result (ref $rt_types__PDict))))
  (type $functype_43 (func (param (ref null $rt_types__PDict) anyref) (result (ref $rt_types__PDict))))
  (type $functype_44 (func (param (ref null $rt_types__Array) (ref null $rt_types__Array)) (result i32)))
  (type $functype_45 (func (param (ref $rt_types__PVec) (ref $rt_types__PVec)) (result i32)))
  (type $functype_46 (func (param (ref $rt_types__PDict) (ref $rt_types__PDict)) (result i32)))
  (type $functype_47 (func (param (ref $rt_types__Variant) (ref $rt_types__Variant)) (result i32)))
  (type $functype_48 (func (param anyref anyref) (result i32)))
  (type $functype_49 (func (param (ref null $rt_types__PVec) (ref null $rt_types__String)) (result (ref null $rt_types__String))))
  (type $functype_50 (func (param (ref null $user__UserRecord_11)) (result i64)))
  (type $functype_51 (func (param (ref null $user__UserRecord_11) i64 i64) (result (ref null $user__UserRecord_11))))
  (type $functype_52 (func))
  (type $functype_53 (func (param anyref anyref) (result anyref)))
  (type $functype_54 (func (param anyref) (result (ref null $rt_types__Variant))))
  (type $functype_55 (func (param (ref null $rt_types__String)) (result anyref)))
  (type $functype_56 (func (param i32) (result anyref)))
  (type $functype_57 (func (param (ref null $rt_types__String)) (result (ref $rt_types__Array))))
  (type $functype_58 (func (param (ref null $rt_types__Array)) (result anyref)))
  (type $functype_59 (func (result (ref $rt_types__String))))
  (import "host" "f64_to_string" (func $rt_str__host_f64_to_string (type $functype_0)))
  (import "host" "print" (func $rt_core__host_print (type $functype_1)))
  (import "host" "println" (func $rt_core__host_println (type $functype_1)))
  (import "host" "error" (func $rt_core__host_error (type $functype_1)))
  (import "host" "eprint" (func $rt_core__host_eprint (type $functype_1)))
  (import "host" "eprintln" (func $rt_core__host_eprintln (type $functype_1)))
  (global $rt_arr__empty_leaf (ref $rt_types__Array) array.new_fixed $rt_types__Array 0)
  (global $rt_arr__empty_pvec (ref $rt_types__PVec) i32.const 0 i32.const 0 ref.null $rt_types__VecInternal global.get $rt_arr__empty_leaf struct.new $rt_types__PVec)
  (global $user____str_lit_global_empty (mut (ref null $rt_types__String)) ref.null $rt_types__String)
  (global $user____str_lit_global_6a6f696e3a20696e76616c69642075746638 (mut (ref null $rt_types__String)) ref.null $rt_types__String)
  (global $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f7072656c7564655f766563746f722e6a6f696e202846756e6349642838352929 (mut (ref null $rt_types__String)) ref.null $rt_types__String)
  (func $rt_arr__tailoff (type $functype_2)
    (param $p0 i32)
    (result i32)
    local.get $p0
    i32.const 32
    i32.le_s
    (if (result i32)
      (then
        i32.const 0)
      (else
        local.get $p0
        i32.const 1
        i32.sub
        i32.const 5
        i32.shr_u
        i32.const 5
        i32.shl))
  )
  (func $rt_arr__get_leaf (type $functype_3)
    (param $p0 (ref $rt_types__PVec))
    (param $p1 i32)
    (result (ref $rt_types__Array))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__VecInternal))
    (local $p5 i32)
    local.get $p0
    struct.get $rt_types__PVec 0
    local.set $p2
    local.get $p2
    i32.const 32
    i32.le_s
    (if (result (ref $rt_types__Array))
      (then
        local.get $p0
        struct.get $rt_types__PVec 3)
      (else
        local.get $p2
        i32.const 1
        i32.sub
        i32.const 5
        i32.shr_u
        i32.const 5
        i32.shl
        local.set $p3
        local.get $p1
        local.get $p3
        i32.ge_s
        (if (result (ref $rt_types__Array))
          (then
            local.get $p0
            struct.get $rt_types__PVec 3)
          (else
            local.get $p0
            struct.get $rt_types__PVec 2
            local.set $p4
            local.get $p0
            struct.get $rt_types__PVec 1
            local.set $p5
            (block $brk
              (loop $lp
                local.get $p5
                i32.const 5
                i32.le_s
                br_if $brk
                local.get $p4
                ref.as_non_null
                struct.get $rt_types__VecInternal 0
                local.get $p1
                local.get $p5
                i32.shr_u
                i32.const 31
                i32.and
                array.get $rt_types__VecChildren
                ref.cast (ref null $rt_types__VecInternal)
                local.set $p4
                local.get $p5
                i32.const 5
                i32.sub
                local.set $p5
                br $lp))
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__VecInternal 0
            local.get $p1
            local.get $p5
            i32.shr_u
            i32.const 31
            i32.and
            array.get $rt_types__VecChildren
            ref.cast (ref $rt_types__Array)))))
  )
  (func $rt_arr__new_path (type $functype_4)
    (param $p0 i32)
    (param $p1 (ref eq))
    (result (ref eq))
    (local $p2 (ref null $rt_types__VecChildren))
    (block $brk
      (loop $lp
        local.get $p0
        i32.eqz
        br_if $brk
        ref.null eq
        i32.const 32
        array.new $rt_types__VecChildren
        local.set $p2
        local.get $p2
        ref.as_non_null
        i32.const 0
        local.get $p1
        array.set $rt_types__VecChildren
        local.get $p2
        ref.as_non_null
        struct.new $rt_types__VecInternal
        ref.cast (ref eq)
        local.set $p1
        local.get $p0
        i32.const 5
        i32.sub
        local.set $p0
        br $lp))
    local.get $p1
  )
  (func $rt_arr__push_tail (type $functype_5)
    (param $p0 i32)
    (param $p1 i32)
    (param $p2 (ref null $rt_types__VecInternal))
    (param $p3 (ref eq))
    (result (ref eq))
    (local $p4 (ref null $rt_types__VecChildren))
    (local $p5 i32)
    (local $p6 (ref null eq))
    local.get $p0
    i32.const 1
    i32.sub
    local.get $p1
    i32.shr_u
    i32.const 31
    i32.and
    local.set $p5
    ref.null eq
    i32.const 32
    array.new $rt_types__VecChildren
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p2
    ref.as_non_null
    struct.get $rt_types__VecInternal 0
    i32.const 0
    i32.const 32
    array.copy $rt_types__VecChildren $rt_types__VecChildren
    local.get $p1
    i32.const 5
    i32.eq
    (if
      (then
        local.get $p4
        ref.as_non_null
        local.get $p5
        local.get $p3
        array.set $rt_types__VecChildren)
      (else
        local.get $p2
        ref.as_non_null
        struct.get $rt_types__VecInternal 0
        local.get $p5
        array.get $rt_types__VecChildren
        local.set $p6
        local.get $p6
        ref.is_null
        (if (result (ref eq))
          (then
            local.get $p1
            i32.const 5
            i32.sub
            local.get $p3
            call $rt_arr__new_path)
          (else
            local.get $p0
            local.get $p1
            i32.const 5
            i32.sub
            local.get $p6
            ref.cast (ref null $rt_types__VecInternal)
            local.get $p3
            call $rt_arr__push_tail))
        local.set $p6
        local.get $p4
        ref.as_non_null
        local.get $p5
        local.get $p6
        array.set $rt_types__VecChildren))
    local.get $p4
    ref.as_non_null
    struct.new $rt_types__VecInternal
    ref.cast (ref eq)
  )
  (func $rt_arr__do_set (type $functype_6)
    (param $p0 i32)
    (param $p1 (ref eq))
    (param $p2 i32)
    (param $p3 anyref)
    (result (ref eq))
    (local $p4 (ref null $rt_types__VecChildren))
    (local $p5 i32)
    (local $p6 (ref null $rt_types__Array))
    (local $p7 (ref null $rt_types__Array))
    local.get $p0
    i32.eqz
    (if (result (ref eq))
      (then
        local.get $p1
        ref.cast (ref $rt_types__Array)
        local.set $p7
        ref.null none
        local.get $p7
        ref.as_non_null
        array.len
        array.new $rt_types__Array
        local.set $p6
        local.get $p6
        ref.as_non_null
        i32.const 0
        local.get $p7
        ref.as_non_null
        i32.const 0
        local.get $p7
        ref.as_non_null
        array.len
        array.copy $rt_types__Array $rt_types__Array
        local.get $p6
        ref.as_non_null
        local.get $p2
        i32.const 31
        i32.and
        local.get $p3
        array.set $rt_types__Array
        local.get $p6
        ref.as_non_null
        ref.cast (ref eq))
      (else
        local.get $p2
        local.get $p0
        i32.shr_u
        i32.const 31
        i32.and
        local.set $p5
        ref.null eq
        i32.const 32
        array.new $rt_types__VecChildren
        local.set $p4
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p1
        ref.cast (ref $rt_types__VecInternal)
        struct.get $rt_types__VecInternal 0
        i32.const 0
        i32.const 32
        array.copy $rt_types__VecChildren $rt_types__VecChildren
        local.get $p4
        ref.as_non_null
        local.get $p5
        local.get $p0
        i32.const 5
        i32.sub
        local.get $p1
        ref.cast (ref $rt_types__VecInternal)
        struct.get $rt_types__VecInternal 0
        local.get $p5
        array.get $rt_types__VecChildren
        ref.as_non_null
        local.get $p2
        local.get $p3
        call $rt_arr__do_set
        array.set $rt_types__VecChildren
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__VecInternal
        ref.cast (ref eq)))
  )
  (func $rt_arr__push (type $functype_7)
    (param $p0 (ref $rt_types__PVec))
    (param $p1 anyref)
    (result (ref $rt_types__PVec))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    (local $p5 (ref null eq))
    (local $p6 i32)
    (local $p7 (ref null $rt_types__VecChildren))
    local.get $p0
    struct.get $rt_types__PVec 0
    local.set $p2
    local.get $p0
    struct.get $rt_types__PVec 3
    array.len
    local.set $p3
    local.get $p3
    i32.const 32
    i32.lt_s
    (if (result (ref $rt_types__PVec))
      (then
        ref.null none
        local.get $p3
        i32.const 1
        i32.add
        array.new $rt_types__Array
        local.set $p4
        local.get $p3
        i32.eqz
        (if
          (then)
          (else
            local.get $p4
            ref.as_non_null
            i32.const 0
            local.get $p0
            struct.get $rt_types__PVec 3
            i32.const 0
            local.get $p3
            array.copy $rt_types__Array $rt_types__Array))
        local.get $p4
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.get $p0
        struct.get $rt_types__PVec 1
        local.get $p0
        struct.get $rt_types__PVec 2
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec)
      (else
        local.get $p0
        struct.get $rt_types__PVec 1
        local.set $p6
        local.get $p0
        struct.get $rt_types__PVec 2
        ref.is_null
        (if
          (then
            i32.const 5
            local.set $p6
            i32.const 5
            local.get $p0
            struct.get $rt_types__PVec 3
            ref.cast (ref eq)
            call $rt_arr__new_path
            local.set $p5)
          (else
            local.get $p2
            i32.const 5
            i32.shr_u
            i32.const 1
            local.get $p6
            i32.shl
            i32.gt_u
            (if
              (then
                ref.null eq
                i32.const 32
                array.new $rt_types__VecChildren
                local.set $p7
                local.get $p7
                ref.as_non_null
                i32.const 0
                local.get $p0
                struct.get $rt_types__PVec 2
                ref.as_non_null
                ref.cast (ref eq)
                array.set $rt_types__VecChildren
                local.get $p7
                ref.as_non_null
                i32.const 1
                local.get $p6
                local.get $p0
                struct.get $rt_types__PVec 3
                ref.cast (ref eq)
                call $rt_arr__new_path
                array.set $rt_types__VecChildren
                local.get $p7
                ref.as_non_null
                struct.new $rt_types__VecInternal
                ref.cast (ref eq)
                local.set $p5
                local.get $p6
                i32.const 5
                i32.add
                local.set $p6)
              (else
                local.get $p2
                local.get $p6
                local.get $p0
                struct.get $rt_types__PVec 2
                local.get $p0
                struct.get $rt_types__PVec 3
                ref.cast (ref eq)
                call $rt_arr__push_tail
                local.set $p5))))
        local.get $p1
        array.new_fixed $rt_types__Array 1
        local.set $p4
        local.get $p2
        i32.const 1
        i32.add
        local.get $p6
        local.get $p5
        ref.cast (ref null $rt_types__VecInternal)
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec))
  )
  (func $rt_arr__make (type $functype_8)
    (param $p0 i32)
    (param $p1 anyref)
    (result (ref $rt_types__PVec))
    (local $p2 (ref null $rt_types__PVec))
    (local $p3 i32)
    local.get $p0
    i32.eqz
    (if (result (ref $rt_types__PVec))
      (then
        global.get $rt_arr__empty_pvec)
      (else
        global.get $rt_arr__empty_pvec
        local.set $p2
        i32.const 0
        local.set $p3
        (block $brk
          (loop $lp
            local.get $p3
            local.get $p0
            i32.ge_s
            br_if $brk
            local.get $p2
            ref.as_non_null
            local.get $p1
            call $rt_arr__push
            local.set $p2
            local.get $p3
            i32.const 1
            i32.add
            local.set $p3
            br $lp))
        local.get $p2
        ref.as_non_null))
  )
  (func $rt_arr__get (type $functype_9)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 i32)
    (result anyref)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__VecInternal))
    (local $p5 i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p2
    local.get $p2
    i32.const 32
    i32.le_s
    (if (result anyref)
      (then
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        local.get $p1
        i32.const 31
        i32.and
        array.get $rt_types__Array)
      (else
        local.get $p2
        i32.const 1
        i32.sub
        i32.const 5
        i32.shr_u
        i32.const 5
        i32.shl
        local.set $p3
        local.get $p1
        local.get $p3
        i32.ge_s
        (if (result anyref)
          (then
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__PVec 3
            local.get $p1
            i32.const 31
            i32.and
            array.get $rt_types__Array)
          (else
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__PVec 2
            local.set $p4
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__PVec 1
            local.set $p5
            (block $brk
              (loop $lp
                local.get $p5
                i32.const 5
                i32.le_s
                br_if $brk
                local.get $p4
                ref.as_non_null
                struct.get $rt_types__VecInternal 0
                local.get $p1
                local.get $p5
                i32.shr_u
                i32.const 31
                i32.and
                array.get $rt_types__VecChildren
                ref.cast (ref null $rt_types__VecInternal)
                local.set $p4
                local.get $p5
                i32.const 5
                i32.sub
                local.set $p5
                br $lp))
            local.get $p4
            ref.as_non_null
            struct.get $rt_types__VecInternal 0
            local.get $p1
            local.get $p5
            i32.shr_u
            i32.const 31
            i32.and
            array.get $rt_types__VecChildren
            ref.cast (ref $rt_types__Array)
            local.get $p1
            i32.const 31
            i32.and
            array.get $rt_types__Array))))
  )
  (func $rt_arr__set (type $functype_10)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 i32)
    (param $p2 anyref)
    (result (ref $rt_types__PVec))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    call $rt_arr__tailoff
    local.set $p3
    local.get $p1
    local.get $p3
    i32.ge_s
    (if (result (ref $rt_types__PVec))
      (then
        ref.null none
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        array.len
        array.new $rt_types__Array
        local.set $p4
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        i32.const 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        array.len
        array.copy $rt_types__Array $rt_types__Array
        local.get $p4
        ref.as_non_null
        local.get $p1
        local.get $p3
        i32.sub
        local.get $p2
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 2
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec)
      (else
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 0
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 2
        ref.as_non_null
        ref.cast (ref eq)
        local.get $p1
        local.get $p2
        call $rt_arr__do_set
        ref.cast (ref null $rt_types__VecInternal)
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__PVec 3
        struct.new $rt_types__PVec))
  )
  (func $rt_arr__len (type $functype_11)
    (param $p0 (ref null $rt_types__PVec))
    (result i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
  )
  (func $rt_arr__concat (type $functype_12)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 (ref null $rt_types__PVec))
    (result (ref $rt_types__PVec))
    (local $p2 (ref null $rt_types__PVec))
    (local $p3 i32)
    (local $p4 i32)
    local.get $p0
    ref.as_non_null
    local.set $p2
    local.get $p1
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p4
    i32.const 0
    local.set $p3
    (block $brk
      (loop $lp
        local.get $p3
        local.get $p4
        i32.ge_s
        br_if $brk
        local.get $p2
        ref.as_non_null
        local.get $p1
        local.get $p3
        call $rt_arr__get
        call $rt_arr__push
        local.set $p2
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $lp))
    local.get $p2
    ref.as_non_null
  )
  (func $rt_arr__slice (type $functype_13)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__PVec))
    (local $p3 (ref null $rt_types__PVec))
    (local $p4 i32)
    global.get $rt_arr__empty_pvec
    local.set $p3
    local.get $p1
    local.set $p4
    (block $brk
      (loop $lp
        local.get $p4
        local.get $p2
        i32.ge_s
        br_if $brk
        local.get $p3
        ref.as_non_null
        local.get $p0
        local.get $p4
        call $rt_arr__get
        call $rt_arr__push
        local.set $p3
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $lp))
    local.get $p3
    ref.as_non_null
  )
  (func $rt_arr__builder_new (type $functype_14)
    (result (ref $rt_types__Array))
    global.get $rt_arr__empty_pvec
    i64.const 0
    struct.new $rt_types__BoxedInt
    ref.null none
    i32.const 32
    array.new $rt_types__Array
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_from (type $functype_15)
    (param $p0 (ref null $rt_types__PVec))
    (result (ref $rt_types__Array))
    (local $p1 (ref null $rt_types__Array))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 (ref null $rt_types__PVec))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 3
    local.set $p1
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p2
    ref.null none
    i32.const 32
    array.new $rt_types__Array
    local.set $p3
    local.get $p2
    i32.eqz
    (if
      (then)
      (else
        local.get $p3
        ref.as_non_null
        i32.const 0
        local.get $p1
        ref.as_non_null
        i32.const 0
        local.get $p2
        array.copy $rt_types__Array $rt_types__Array))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.get $p2
    i32.sub
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 1
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 2
    global.get $rt_arr__empty_leaf
    struct.new $rt_types__PVec
    local.set $p4
    local.get $p4
    ref.as_non_null
    local.get $p2
    i64.extend_i32_s
    struct.new $rt_types__BoxedInt
    local.get $p3
    array.new_fixed $rt_types__Array 3
  )
  (func $rt_arr__builder_push (type $functype_16)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 anyref)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__PVec))
    (local $p5 (ref null $rt_types__PVec))
    local.get $p0
    ref.as_non_null
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p2
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p3
    local.get $p3
    i32.const 32
    i32.lt_s
    (if
      (then
        local.get $p2
        ref.as_non_null
        local.get $p3
        local.get $p1
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 1
        local.get $p3
        i32.const 1
        i32.add
        i64.extend_i32_s
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array)
      (else
        local.get $p0
        ref.as_non_null
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__PVec)
        local.set $p4
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__PVec 0
        i32.const 32
        i32.add
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p4
        ref.as_non_null
        struct.get $rt_types__PVec 2
        local.get $p2
        ref.as_non_null
        struct.new $rt_types__PVec
        local.get $p1
        call $rt_arr__push
        local.set $p5
        local.get $p0
        ref.as_non_null
        i32.const 0
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__PVec 0
        i32.const 1
        i32.sub
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p5
        ref.as_non_null
        struct.get $rt_types__PVec 2
        global.get $rt_arr__empty_leaf
        struct.new $rt_types__PVec
        array.set $rt_types__Array
        ref.null none
        i32.const 32
        array.new $rt_types__Array
        local.set $p2
        local.get $p2
        ref.as_non_null
        i32.const 0
        local.get $p1
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 2
        local.get $p2
        array.set $rt_types__Array
        local.get $p0
        ref.as_non_null
        i32.const 1
        i64.const 1
        struct.new $rt_types__BoxedInt
        array.set $rt_types__Array))
  )
  (func $rt_arr__builder_extend (type $functype_17)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 (ref null $rt_types__PVec))
    (local $p2 i32)
    (local $p3 i32)
    local.get $p1
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p3
    i32.const 0
    local.set $p2
    (block $brk
      (loop $lp
        local.get $p2
        local.get $p3
        i32.ge_s
        br_if $brk
        local.get $p0
        local.get $p1
        local.get $p2
        call $rt_arr__get
        call $rt_arr__builder_push
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $lp))
  )
  (func $rt_arr__builder_freeze (type $functype_18)
    (param $p0 (ref null $rt_types__Array))
    (result (ref $rt_types__PVec))
    (local $p1 (ref null $rt_types__PVec))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__PVec)
    local.set $p1
    local.get $p0
    ref.as_non_null
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    i32.wrap_i64
    local.set $p2
    local.get $p0
    ref.as_non_null
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__Array)
    local.set $p3
    local.get $p2
    i32.eqz
    (if (result (ref $rt_types__PVec))
      (then
        local.get $p1
        ref.as_non_null)
      (else
        ref.null none
        local.get $p2
        array.new $rt_types__Array
        local.set $p4
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p3
        ref.as_non_null
        i32.const 0
        local.get $p2
        array.copy $rt_types__Array $rt_types__Array
        local.get $p1
        ref.as_non_null
        struct.get $rt_types__PVec 0
        local.get $p2
        i32.add
        local.get $p1
        ref.as_non_null
        struct.get $rt_types__PVec 1
        local.get $p1
        ref.as_non_null
        struct.get $rt_types__PVec 2
        local.get $p4
        ref.as_non_null
        struct.new $rt_types__PVec))
  )
  (func $rt_arr__from_array (type $functype_19)
    (param $p0 (ref $rt_types__Array))
    (result (ref $rt_types__PVec))
    (local $p1 i32)
    (local $p2 (ref null $rt_types__PVec))
    (local $p3 i32)
    local.get $p0
    array.len
    local.set $p1
    local.get $p1
    i32.eqz
    (if (result (ref $rt_types__PVec))
      (then
        global.get $rt_arr__empty_pvec)
      (else
        local.get $p1
        i32.const 32
        i32.le_s
        (if (result (ref $rt_types__PVec))
          (then
            local.get $p1
            i32.const 0
            ref.null $rt_types__VecInternal
            local.get $p0
            struct.new $rt_types__PVec)
          (else
            global.get $rt_arr__empty_pvec
            local.set $p2
            i32.const 0
            local.set $p3
            (block $brk
              (loop $lp
                local.get $p3
                local.get $p1
                i32.ge_s
                br_if $brk
                local.get $p2
                ref.as_non_null
                local.get $p0
                local.get $p3
                array.get $rt_types__Array
                call $rt_arr__push
                local.set $p2
                local.get $p3
                i32.const 1
                i32.add
                local.set $p3
                br $lp))
            local.get $p2
            ref.as_non_null))))
  )
  (func $rt_arr__to_array (type $functype_15)
    (param $p0 (ref null $rt_types__PVec))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PVec 0
    local.set $p1
    ref.null none
    local.get $p1
    array.new $rt_types__Array
    local.set $p2
    i32.const 0
    local.set $p3
    (block $brk
      (loop $lp
        local.get $p3
        local.get $p1
        i32.ge_s
        br_if $brk
        local.get $p2
        ref.as_non_null
        local.get $p3
        local.get $p0
        ref.as_non_null
        local.get $p3
        call $rt_arr__get
        array.set $rt_types__Array
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $lp))
    local.get $p2
    ref.as_non_null
  )
  (func $rt_arr__from_read_file_result (type $functype_20)
    (param $p0 (ref null $rt_types__Variant))
    (result (ref null $rt_types__Variant))
    (local $p1 (ref null $rt_types__Variant))
    (local $p2 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__Variant 0
    i32.const 1
    i32.eq
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__Variant 1
        i32.eqz
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__Variant 2
            local.set $p2
            i32.const 1
            i32.const 0
            local.get $p2
            ref.as_non_null
            i32.const 0
            array.get $rt_types__Array
            ref.cast (ref $rt_types__Array)
            call $rt_arr__from_array
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant)
          (else
            local.get $p0)))
      (else
        local.get $p0))
  )
  (func $rt_str__len (type $functype_21)
    (param $p0 (ref null $rt_types__String))
    (result i32)
    local.get $p0
    ref.as_non_null
    array.len
  )
  (func $rt_str__concat (type $functype_22)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result (ref $rt_types__String))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__String))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p2
    local.get $p3
    i32.add
    local.set $p4
    i32.const 0
    local.get $p4
    array.new $rt_types__String
    local.set $p5
    local.get $p5
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    i32.const 0
    local.get $p2
    array.copy $rt_types__String $rt_types__String
    local.get $p5
    ref.as_non_null
    local.get $p2
    local.get $p1
    ref.as_non_null
    i32.const 0
    local.get $p3
    array.copy $rt_types__String $rt_types__String
    local.get $p5
    ref.as_non_null
  )
  (func $rt_str__substring (type $functype_23)
    (param $p0 (ref null $rt_types__String))
    (param $p1 i32)
    (param $p2 i32)
    (result (ref $rt_types__String))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__String))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p1
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p1))
    local.get $p1
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p1))
    local.get $p2
    i32.const 0
    i32.lt_s
    (if
      (then
        local.get $p3
        local.set $p2))
    local.get $p2
    local.get $p3
    i32.gt_s
    (if
      (then
        local.get $p3
        local.set $p2))
    local.get $p2
    local.get $p1
    i32.lt_s
    (if
      (then
        local.get $p1
        local.set $p2))
    local.get $p2
    local.get $p1
    i32.sub
    local.set $p3
    i32.const 0
    local.get $p3
    array.new $rt_types__String
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p0
    ref.as_non_null
    local.get $p1
    local.get $p3
    array.copy $rt_types__String $rt_types__String
    local.get $p4
    ref.as_non_null
  )
  (func $rt_str__eq (type $functype_24)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p2
    local.get $p1
    ref.as_non_null
    array.len
    i32.ne
    (if
      (then
        i32.const 0
        return))
    i32.const 0
    local.set $p3
    (block $exit
      (loop $cmp
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        i32.ne
        (if
          (then
            i32.const 0
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp))
    i32.const 1
  )
  (func $rt_str__cmp (type $functype_24)
    (param $p0 (ref null $rt_types__String))
    (param $p1 (ref null $rt_types__String))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 i32)
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p6
    local.get $p1
    ref.as_non_null
    array.len
    local.set $p7
    local.get $p6
    local.get $p7
    local.get $p6
    local.get $p7
    i32.le_s
    select
    local.set $p2
    i32.const 0
    local.set $p3
    (block $done
      (loop $cmp_loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $done
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.set $p4
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.set $p5
        local.get $p4
        local.get $p5
        i32.lt_u
        (if
          (then
            i32.const -1
            return))
        local.get $p4
        local.get $p5
        i32.gt_u
        (if
          (then
            i32.const 1
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $cmp_loop))
    local.get $p6
    local.get $p7
    i32.lt_s
    (if
      (then
        i32.const -1
        return))
    local.get $p6
    local.get $p7
    i32.gt_s
    (if
      (then
        i32.const 1
        return))
    i32.const 0
  )
  (func $rt_str__from_i64 (type $functype_25)
    (param $p0 i64)
    (result (ref $rt_types__String))
    (local $p1 i32)
    (local $p2 i64)
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__String))
    (local $p6 (ref null $rt_types__String))
    local.get $p0
    i64.eqz
    (if
      (then
        i32.const 48
        array.new_fixed $rt_types__String 1
        return))
    local.get $p0
    i64.const -9223372036854775808
    i64.eq
    (if
      (then
        i32.const 45
        i32.const 57
        i32.const 50
        i32.const 50
        i32.const 51
        i32.const 51
        i32.const 55
        i32.const 50
        i32.const 48
        i32.const 51
        i32.const 54
        i32.const 56
        i32.const 53
        i32.const 52
        i32.const 55
        i32.const 55
        i32.const 53
        i32.const 56
        i32.const 48
        i32.const 56
        array.new_fixed $rt_types__String 20
        return))
    local.get $p0
    i64.const 0
    i64.lt_s
    local.set $p1
    local.get $p1
    (if (result i64)
      (then
        i64.const 0
        local.get $p0
        i64.sub)
      (else
        local.get $p0))
    local.set $p2
    i32.const 0
    i32.const 20
    array.new $rt_types__String
    local.set $p5
    i32.const 19
    local.set $p3
    (loop $digits
      local.get $p5
      ref.as_non_null
      local.get $p3
      local.get $p2
      i64.const 10
      i64.rem_s
      i32.wrap_i64
      i32.const 48
      i32.add
      array.set $rt_types__String
      local.get $p2
      i64.const 10
      i64.div_s
      local.set $p2
      local.get $p3
      i32.const 1
      i32.sub
      local.set $p3
      local.get $p2
      i64.eqz
      i32.eqz
      br_if $digits)
    local.get $p1
    (if
      (then
        local.get $p5
        ref.as_non_null
        local.get $p3
        i32.const 45
        array.set $rt_types__String
        local.get $p3
        i32.const 1
        i32.sub
        local.set $p3))
    i32.const 19
    local.get $p3
    i32.sub
    local.set $p4
    i32.const 0
    local.get $p4
    array.new $rt_types__String
    local.set $p6
    local.get $p6
    ref.as_non_null
    i32.const 0
    local.get $p5
    ref.as_non_null
    local.get $p3
    i32.const 1
    i32.add
    local.get $p4
    array.copy $rt_types__String $rt_types__String
    local.get $p6
    ref.as_non_null
  )
  (func $rt_str__from_f64 (type $functype_0)
    (param $p0 f64)
    (result (ref $rt_types__String))
    local.get $p0
    call $rt_str__host_f64_to_string
  )
  (func $rt_str__from_bool (type $functype_26)
    (param $p0 i32)
    (result (ref $rt_types__String))
    local.get $p0
    (if (result (ref $rt_types__String))
      (then
        i32.const 116
        i32.const 114
        i32.const 117
        i32.const 101
        array.new_fixed $rt_types__String 4)
      (else
        i32.const 102
        i32.const 97
        i32.const 108
        i32.const 115
        i32.const 101
        array.new_fixed $rt_types__String 5))
  )
  (func $rt_dict__popcount (type $functype_2)
    (param $p0 i32)
    (result i32)
    (local $p1 i32)
    i32.const 0
    local.set $p1
    (block $exit
      (loop $loop
        local.get $p0
        i32.eqz
        br_if $exit
        local.get $p0
        local.get $p0
        i32.const 1
        i32.sub
        i32.and
        local.set $p0
        local.get $p1
        i32.const 1
        i32.add
        local.set $p1
        br $loop))
    local.get $p1
  )
  (func $rt_dict__arr_insert_at (type $functype_27)
    (param $p0 (ref $rt_types__Array))
    (param $p1 i32)
    (param $p2 anyref)
    (result (ref $rt_types__Array))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    (local $p5 i32)
    local.get $p0
    array.len
    local.set $p3
    ref.null any
    local.get $p3
    i32.const 1
    i32.add
    array.new $rt_types__Array
    local.set $p4
    local.get $p1
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p4
        ref.as_non_null
        i32.const 0
        local.get $p0
        i32.const 0
        local.get $p1
        array.copy $rt_types__Array $rt_types__Array))
    local.get $p4
    ref.as_non_null
    local.get $p1
    local.get $p2
    array.set $rt_types__Array
    local.get $p3
    local.get $p1
    i32.sub
    local.set $p5
    local.get $p5
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p4
        ref.as_non_null
        local.get $p1
        i32.const 1
        i32.add
        local.get $p0
        local.get $p1
        local.get $p5
        array.copy $rt_types__Array $rt_types__Array))
    local.get $p4
    ref.as_non_null
  )
  (func $rt_dict__arr_replace_at (type $functype_27)
    (param $p0 (ref $rt_types__Array))
    (param $p1 i32)
    (param $p2 anyref)
    (result (ref $rt_types__Array))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__Array))
    local.get $p0
    array.len
    local.set $p3
    ref.null any
    local.get $p3
    array.new $rt_types__Array
    local.set $p4
    local.get $p4
    ref.as_non_null
    i32.const 0
    local.get $p0
    i32.const 0
    local.get $p3
    array.copy $rt_types__Array $rt_types__Array
    local.get $p4
    ref.as_non_null
    local.get $p1
    local.get $p2
    array.set $rt_types__Array
    local.get $p4
    ref.as_non_null
  )
  (func $rt_dict__arr_remove_at (type $functype_28)
    (param $p0 (ref $rt_types__Array))
    (param $p1 i32)
    (result (ref $rt_types__Array))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__Array))
    (local $p4 i32)
    local.get $p0
    array.len
    local.set $p2
    ref.null any
    local.get $p2
    i32.const 1
    i32.sub
    array.new $rt_types__Array
    local.set $p3
    local.get $p1
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p3
        ref.as_non_null
        i32.const 0
        local.get $p0
        i32.const 0
        local.get $p1
        array.copy $rt_types__Array $rt_types__Array))
    local.get $p2
    local.get $p1
    i32.sub
    i32.const 1
    i32.sub
    local.set $p4
    local.get $p4
    i32.const 0
    i32.gt_s
    (if
      (then
        local.get $p3
        ref.as_non_null
        local.get $p1
        local.get $p0
        local.get $p1
        i32.const 1
        i32.add
        local.get $p4
        array.copy $rt_types__Array $rt_types__Array))
    local.get $p3
    ref.as_non_null
  )
  (func $rt_dict__hash_i64 (type $functype_29)
    (param $p0 i64)
    (result i32)
    (local $p1 i32)
    local.get $p0
    i32.wrap_i64
    local.get $p0
    i64.const 32
    i64.shr_s
    i32.wrap_i64
    i32.xor
    local.set $p1
    local.get $p1
    i32.const -1640531527
    i32.mul
  )
  (func $rt_dict__hash_string (type $functype_21)
    (param $p0 (ref null $rt_types__String))
    (result i32)
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 i32)
    i32.const -2128831035
    local.set $p1
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    i32.const 0
    local.set $p3
    (block $exit
      (loop $loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get_u $rt_types__String
        local.get $p1
        i32.xor
        i32.const 16777619
        i32.mul
        local.set $p1
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $loop))
    local.get $p1
  )
  (func $rt_dict__hash_key (type $functype_30)
    (param $p0 anyref)
    (result i32)
    local.get $p0
    ref.test (ref i31)
    (if
      (then
        local.get $p0
        ref.cast (ref i31)
        i31.get_u
        i64.extend_i32_u
        call $rt_dict__hash_i64
        return))
    local.get $p0
    ref.test (ref $rt_types__BoxedInt)
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        call $rt_dict__hash_i64
        return))
    local.get $p0
    ref.cast (ref null $rt_types__String)
    call $rt_dict__hash_string
  )
  (func $rt_dict__collision_get (type $functype_31)
    (param $p0 (ref null $rt_types__HamtCollision))
    (param $p1 anyref)
    (result anyref)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 i32)
    (local $p4 i32)
    (local $p5 (ref null $rt_types__HamtEntry))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtCollision 1
    local.set $p2
    local.get $p2
    ref.as_non_null
    array.len
    local.set $p3
    i32.const 0
    local.set $p4
    (block $exit
      (loop $scan
        local.get $p4
        local.get $p3
        i32.ge_s
        br_if $exit
        local.get $p2
        ref.as_non_null
        local.get $p4
        array.get $rt_types__Array
        ref.cast (ref $rt_types__HamtEntry)
        local.set $p5
        local.get $p5
        struct.get $rt_types__HamtEntry 1
        local.get $p1
        call $rt_core__eq
        (if
          (then
            local.get $p5
            struct.get $rt_types__HamtEntry 2
            return))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $scan))
    ref.null any
  )
  (func $rt_dict__collision_set (type $functype_32)
    (param $p0 (ref null $rt_types__HamtCollision))
    (param $p1 i32)
    (param $p2 anyref)
    (param $p3 anyref)
    (result (ref $rt_types__HamtCollision))
    (local $p4 (ref null $rt_types__Array))
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 (ref null $rt_types__HamtEntry))
    (local $p8 (ref null $rt_types__Array))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtCollision 1
    local.set $p4
    local.get $p4
    ref.as_non_null
    array.len
    local.set $p5
    i32.const 0
    local.set $p6
    (block $found_exit
      (loop $scan
        local.get $p6
        local.get $p5
        i32.ge_s
        br_if $found_exit
        local.get $p4
        ref.as_non_null
        local.get $p6
        array.get $rt_types__Array
        ref.cast (ref $rt_types__HamtEntry)
        local.set $p7
        local.get $p7
        struct.get $rt_types__HamtEntry 1
        local.get $p2
        call $rt_core__eq
        (if
          (then
            local.get $p1
            local.get $p2
            local.get $p3
            struct.new $rt_types__HamtEntry
            local.set $p7
            local.get $p4
            ref.as_non_null
            local.get $p6
            local.get $p7
            call $rt_dict__arr_replace_at
            local.set $p8
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__HamtCollision 0
            local.get $p8
            ref.as_non_null
            struct.new $rt_types__HamtCollision
            return))
        local.get $p6
        i32.const 1
        i32.add
        local.set $p6
        br $scan))
    local.get $p1
    local.get $p2
    local.get $p3
    struct.new $rt_types__HamtEntry
    local.set $p7
    local.get $p4
    ref.as_non_null
    local.get $p5
    local.get $p7
    call $rt_dict__arr_insert_at
    local.set $p8
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtCollision 0
    local.get $p8
    ref.as_non_null
    struct.new $rt_types__HamtCollision
  )
  (func $rt_dict__node_get (type $functype_33)
    (param $p0 (ref null $rt_types__HamtNode))
    (param $p1 i32)
    (param $p2 i32)
    (param $p3 anyref)
    (result anyref)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 i32)
    (local $p8 anyref)
    local.get $p0
    ref.is_null
    (if
      (then
        ref.null any
        return))
    local.get $p1
    local.get $p2
    i32.const 5
    i32.mul
    i32.shr_u
    i32.const 31
    i32.and
    local.set $p4
    i32.const 1
    local.get $p4
    i32.shl
    local.set $p5
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 0
    local.set $p6
    local.get $p6
    local.get $p5
    i32.and
    i32.eqz
    (if
      (then
        ref.null any
        return))
    local.get $p6
    local.get $p5
    i32.const 1
    i32.sub
    i32.and
    call $rt_dict__popcount
    local.set $p7
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 1
    local.get $p7
    array.get $rt_types__Array
    local.set $p8
    local.get $p8
    ref.test (ref $rt_types__HamtEntry)
    (if
      (then
        local.get $p8
        ref.cast (ref $rt_types__HamtEntry)
        struct.get $rt_types__HamtEntry 0
        local.get $p1
        i32.eq
        (if
          (then
            local.get $p8
            ref.cast (ref $rt_types__HamtEntry)
            struct.get $rt_types__HamtEntry 1
            local.get $p3
            call $rt_core__eq
            (if
              (then
                local.get $p8
                ref.cast (ref $rt_types__HamtEntry)
                struct.get $rt_types__HamtEntry 2
                return))))
        ref.null any
        return))
    local.get $p8
    ref.test (ref $rt_types__HamtNode)
    (if
      (then
        local.get $p8
        ref.cast (ref null $rt_types__HamtNode)
        local.get $p1
        local.get $p2
        i32.const 1
        i32.add
        local.get $p3
        call $rt_dict__node_get
        return))
    local.get $p8
    ref.cast (ref null $rt_types__HamtCollision)
    local.get $p3
    call $rt_dict__collision_get
  )
  (func $rt_dict__node_set (type $functype_34)
    (param $p0 (ref null $rt_types__HamtNode))
    (param $p1 i32)
    (param $p2 i32)
    (param $p3 anyref)
    (param $p4 anyref)
    (result (ref $rt_types__HamtNode))
    (local $p5 (ref null $rt_types__HamtEntry))
    (local $p6 i32)
    (local $p7 i32)
    (local $p8 i32)
    (local $p9 i32)
    (local $p10 i32)
    (local $p11 anyref)
    (local $p12 (ref null $rt_types__Array))
    (local $p13 (ref null $rt_types__HamtEntry))
    (local $p14 (ref null $rt_types__HamtNode))
    (local $p15 (ref null $rt_types__HamtCollision))
    (local $p16 (ref null $rt_types__HamtCollision))
    local.get $p1
    local.get $p3
    local.get $p4
    struct.new $rt_types__HamtEntry
    local.set $p5
    local.get $p0
    ref.is_null
    (if
      (then
        local.get $p1
        local.get $p2
        i32.const 5
        i32.mul
        i32.shr_u
        i32.const 31
        i32.and
        local.set $p7
        i32.const 1
        local.get $p7
        i32.shl
        local.set $p8
        local.get $p8
        local.get $p5
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__HamtNode
        return))
    local.get $p2
    i32.const 5
    i32.mul
    local.set $p6
    local.get $p1
    local.get $p6
    i32.shr_u
    i32.const 31
    i32.and
    local.set $p7
    i32.const 1
    local.get $p7
    i32.shl
    local.set $p8
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 0
    local.set $p9
    local.get $p9
    local.get $p8
    i32.const 1
    i32.sub
    i32.and
    call $rt_dict__popcount
    local.set $p10
    local.get $p9
    local.get $p8
    i32.and
    i32.eqz
    (if
      (then
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__HamtNode 1
        local.get $p10
        local.get $p5
        call $rt_dict__arr_insert_at
        local.set $p12
        local.get $p9
        local.get $p8
        i32.or
        local.get $p12
        ref.as_non_null
        struct.new $rt_types__HamtNode
        return))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 1
    local.get $p10
    array.get $rt_types__Array
    local.set $p11
    local.get $p11
    ref.test (ref $rt_types__HamtEntry)
    (if
      (then
        local.get $p11
        ref.cast (ref $rt_types__HamtEntry)
        local.set $p13
        local.get $p13
        struct.get $rt_types__HamtEntry 0
        local.get $p1
        i32.eq
        (if
          (then
            local.get $p13
            struct.get $rt_types__HamtEntry 1
            local.get $p3
            call $rt_core__eq
            (if
              (then
                local.get $p0
                ref.as_non_null
                struct.get $rt_types__HamtNode 1
                local.get $p10
                local.get $p5
                call $rt_dict__arr_replace_at
                local.set $p12
                local.get $p9
                local.get $p12
                ref.as_non_null
                struct.new $rt_types__HamtNode
                return)
              (else
                local.get $p13
                local.get $p5
                array.new_fixed $rt_types__Array 2
                local.set $p12
                local.get $p1
                local.get $p12
                ref.as_non_null
                struct.new $rt_types__HamtCollision
                local.set $p15
                local.get $p0
                ref.as_non_null
                struct.get $rt_types__HamtNode 1
                local.get $p10
                local.get $p15
                call $rt_dict__arr_replace_at
                local.set $p12
                local.get $p9
                local.get $p12
                ref.as_non_null
                struct.new $rt_types__HamtNode
                return)))
          (else
            ref.null $rt_types__HamtNode
            local.get $p13
            struct.get $rt_types__HamtEntry 0
            local.get $p2
            i32.const 1
            i32.add
            local.get $p13
            struct.get $rt_types__HamtEntry 1
            local.get $p13
            struct.get $rt_types__HamtEntry 2
            call $rt_dict__node_set
            local.set $p14
            local.get $p14
            local.get $p1
            local.get $p2
            i32.const 1
            i32.add
            local.get $p3
            local.get $p4
            call $rt_dict__node_set
            local.set $p14
            local.get $p0
            ref.as_non_null
            struct.get $rt_types__HamtNode 1
            local.get $p10
            local.get $p14
            call $rt_dict__arr_replace_at
            local.set $p12
            local.get $p9
            local.get $p12
            ref.as_non_null
            struct.new $rt_types__HamtNode
            return))))
    local.get $p11
    ref.test (ref $rt_types__HamtNode)
    (if
      (then
        local.get $p11
        ref.cast (ref null $rt_types__HamtNode)
        local.get $p1
        local.get $p2
        i32.const 1
        i32.add
        local.get $p3
        local.get $p4
        call $rt_dict__node_set
        local.set $p14
        local.get $p0
        ref.as_non_null
        struct.get $rt_types__HamtNode 1
        local.get $p10
        local.get $p14
        call $rt_dict__arr_replace_at
        local.set $p12
        local.get $p9
        local.get $p12
        ref.as_non_null
        struct.new $rt_types__HamtNode
        return))
    local.get $p11
    ref.cast (ref null $rt_types__HamtCollision)
    local.set $p15
    local.get $p15
    local.get $p1
    local.get $p3
    local.get $p4
    call $rt_dict__collision_set
    local.set $p16
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 1
    local.get $p10
    local.get $p16
    call $rt_dict__arr_replace_at
    local.set $p12
    local.get $p9
    local.get $p12
    ref.as_non_null
    struct.new $rt_types__HamtNode
  )
  (func $rt_dict__node_remove (type $functype_35)
    (param $p0 (ref null $rt_types__HamtNode))
    (param $p1 i32)
    (param $p2 i32)
    (param $p3 anyref)
    (result (ref null $rt_types__HamtNode))
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 i32)
    (local $p8 anyref)
    (local $p9 (ref null $rt_types__Array))
    (local $p10 i32)
    (local $p11 (ref null $rt_types__HamtEntry))
    (local $p12 (ref null $rt_types__Array))
    (local $p13 (ref null $rt_types__HamtNode))
    (local $p14 (ref null $rt_types__HamtNode))
    (local $p15 (ref null $rt_types__HamtCollision))
    (local $p16 (ref null $rt_types__Array))
    (local $p17 i32)
    (local $p18 i32)
    (local $p19 i32)
    (local $p20 (ref null $rt_types__HamtEntry))
    (local $p21 (ref null $rt_types__HamtCollision))
    local.get $p0
    ref.is_null
    (if
      (then
        ref.null $rt_types__HamtNode
        return))
    local.get $p1
    local.get $p2
    i32.const 5
    i32.mul
    i32.shr_u
    i32.const 31
    i32.and
    local.set $p4
    i32.const 1
    local.get $p4
    i32.shl
    local.set $p5
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 0
    local.set $p6
    local.get $p6
    local.get $p5
    i32.and
    i32.eqz
    (if
      (then
        local.get $p0
        return))
    local.get $p6
    local.get $p5
    i32.const 1
    i32.sub
    i32.and
    call $rt_dict__popcount
    local.set $p7
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__HamtNode 1
    local.set $p9
    local.get $p9
    ref.as_non_null
    array.len
    local.set $p10
    local.get $p9
    ref.as_non_null
    local.get $p7
    array.get $rt_types__Array
    local.set $p8
    local.get $p8
    ref.test (ref $rt_types__HamtEntry)
    (if
      (then
        local.get $p8
        ref.cast (ref $rt_types__HamtEntry)
        local.set $p11
        local.get $p11
        struct.get $rt_types__HamtEntry 0
        local.get $p1
        i32.eq
        (if
          (then
            local.get $p11
            struct.get $rt_types__HamtEntry 1
            local.get $p3
            call $rt_core__eq
            (if
              (then
                local.get $p10
                i32.const 1
                i32.eq
                (if
                  (then
                    ref.null $rt_types__HamtNode
                    return))
                local.get $p9
                ref.as_non_null
                local.get $p7
                call $rt_dict__arr_remove_at
                local.set $p12
                local.get $p6
                local.get $p5
                i32.sub
                local.get $p12
                ref.as_non_null
                struct.new $rt_types__HamtNode
                return)
              (else
                local.get $p0
                return)))
          (else
            local.get $p0
            return))))
    local.get $p8
    ref.test (ref $rt_types__HamtNode)
    (if
      (then
        local.get $p8
        ref.cast (ref null $rt_types__HamtNode)
        local.set $p13
        local.get $p13
        local.get $p1
        local.get $p2
        i32.const 1
        i32.add
        local.get $p3
        call $rt_dict__node_remove
        local.set $p14
        local.get $p14
        ref.is_null
        (if
          (then
            local.get $p10
            i32.const 1
            i32.eq
            (if
              (then
                ref.null $rt_types__HamtNode
                return))
            local.get $p9
            ref.as_non_null
            local.get $p7
            call $rt_dict__arr_remove_at
            local.set $p12
            local.get $p6
            local.get $p5
            i32.sub
            local.get $p12
            ref.as_non_null
            struct.new $rt_types__HamtNode
            return))
        local.get $p9
        ref.as_non_null
        local.get $p7
        local.get $p14
        call $rt_dict__arr_replace_at
        local.set $p12
        local.get $p6
        local.get $p12
        ref.as_non_null
        struct.new $rt_types__HamtNode
        return))
    local.get $p8
    ref.cast (ref null $rt_types__HamtCollision)
    local.set $p15
    local.get $p15
    ref.as_non_null
    struct.get $rt_types__HamtCollision 1
    local.set $p16
    local.get $p16
    ref.as_non_null
    array.len
    local.set $p17
    i32.const -1
    local.set $p18
    i32.const 0
    local.set $p19
    (block $find_exit
      (loop $find
        local.get $p19
        local.get $p17
        i32.ge_s
        br_if $find_exit
        local.get $p16
        ref.as_non_null
        local.get $p19
        array.get $rt_types__Array
        ref.cast (ref $rt_types__HamtEntry)
        local.set $p20
        local.get $p20
        struct.get $rt_types__HamtEntry 1
        local.get $p3
        call $rt_core__eq
        (if
          (then
            local.get $p19
            local.set $p18
            br $find_exit))
        local.get $p19
        i32.const 1
        i32.add
        local.set $p19
        br $find))
    local.get $p18
    i32.const 1
    i32.add
    i32.eqz
    (if
      (then
        local.get $p0
        return))
    local.get $p16
    ref.as_non_null
    local.get $p18
    call $rt_dict__arr_remove_at
    local.set $p12
    local.get $p12
    ref.as_non_null
    array.len
    i32.const 1
    i32.eq
    (if
      (then
        local.get $p12
        ref.as_non_null
        i32.const 0
        array.get $rt_types__Array
        local.set $p8
        local.get $p9
        ref.as_non_null
        local.get $p7
        local.get $p8
        call $rt_dict__arr_replace_at
        local.set $p12
        local.get $p6
        local.get $p12
        ref.as_non_null
        struct.new $rt_types__HamtNode
        return))
    local.get $p15
    ref.as_non_null
    struct.get $rt_types__HamtCollision 0
    local.get $p12
    ref.as_non_null
    struct.new $rt_types__HamtCollision
    local.set $p21
    local.get $p9
    ref.as_non_null
    local.get $p7
    local.get $p21
    call $rt_dict__arr_replace_at
    local.set $p12
    local.get $p6
    local.get $p12
    ref.as_non_null
    struct.new $rt_types__HamtNode
  )
  (func $rt_dict__order_remove_key (type $functype_7)
    (param $p0 (ref $rt_types__PVec))
    (param $p1 anyref)
    (result (ref $rt_types__PVec))
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 (ref null $rt_types__PVec))
    (local $p5 anyref)
    local.get $p0
    call $rt_arr__len
    local.set $p2
    i32.const 0
    i32.const 0
    ref.null $rt_types__VecInternal
    array.new_fixed $rt_types__Array 0
    struct.new $rt_types__PVec
    local.set $p4
    i32.const 0
    local.set $p3
    (block $exit
      (loop $loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        local.get $p3
        call $rt_arr__get
        local.set $p5
        local.get $p5
        local.get $p1
        call $rt_core__eq
        i32.eqz
        (if
          (then
            local.get $p4
            ref.as_non_null
            local.get $p5
            call $rt_arr__push
            local.set $p4))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $loop))
    local.get $p4
    ref.as_non_null
  )
  (func $rt_dict__make (type $functype_36)
    (result (ref $rt_types__PDict))
    i32.const 0
    ref.null $rt_types__HamtNode
    i32.const 0
    i32.const 0
    ref.null $rt_types__VecInternal
    array.new_fixed $rt_types__Array 0
    struct.new $rt_types__PVec
    struct.new $rt_types__PDict
  )
  (func $rt_dict__len (type $functype_37)
    (param $p0 (ref null $rt_types__PDict))
    (result i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 0
  )
  (func $rt_dict__keys (type $functype_38)
    (param $p0 (ref null $rt_types__PDict))
    (result (ref $rt_types__PVec))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 2
  )
  (func $rt_dict__has (type $functype_39)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (result i32)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 1
    local.get $p1
    call $rt_dict__hash_key
    i32.const 0
    local.get $p1
    call $rt_dict__node_get
    ref.is_null
    i32.eqz
  )
  (func $rt_dict__get (type $functype_40)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (result anyref)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 1
    local.get $p1
    call $rt_dict__hash_key
    i32.const 0
    local.get $p1
    call $rt_dict__node_get
  )
  (func $rt_dict__get_option (type $functype_41)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (result (ref $rt_types__Variant))
    (local $p2 anyref)
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 1
    local.get $p1
    call $rt_dict__hash_key
    i32.const 0
    local.get $p1
    call $rt_dict__node_get
    local.set $p2
    local.get $p2
    ref.is_null
    (if (result (ref $rt_types__Variant))
      (then
        i32.const 0
        i32.const 0
        ref.null $rt_types__Array
        struct.new $rt_types__Variant)
      (else
        i32.const 0
        i32.const 1
        local.get $p2
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
  )
  (func $rt_dict__set (type $functype_42)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref $rt_types__PDict))
    (local $p3 i32)
    (local $p4 (ref null $rt_types__HamtNode))
    (local $p5 (ref null $rt_types__HamtNode))
    (local $p6 i32)
    (local $p7 (ref null $rt_types__PVec))
    local.get $p1
    call $rt_dict__hash_key
    local.set $p3
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 1
    local.set $p4
    local.get $p4
    local.get $p3
    i32.const 0
    local.get $p1
    call $rt_dict__node_get
    ref.is_null
    i32.eqz
    local.set $p6
    local.get $p4
    local.get $p3
    i32.const 0
    local.get $p1
    local.get $p2
    call $rt_dict__node_set
    local.set $p5
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 2
    local.set $p7
    local.get $p6
    i32.eqz
    (if
      (then
        local.get $p7
        ref.as_non_null
        local.get $p1
        call $rt_arr__push
        local.set $p7))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 0
    local.get $p6
    i32.eqz
    i32.add
    local.get $p5
    local.get $p7
    ref.as_non_null
    struct.new $rt_types__PDict
  )
  (func $rt_dict__remove (type $functype_43)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (result (ref $rt_types__PDict))
    (local $p2 i32)
    (local $p3 (ref null $rt_types__HamtNode))
    (local $p4 (ref null $rt_types__HamtNode))
    (local $p5 i32)
    (local $p6 (ref null $rt_types__PVec))
    local.get $p1
    call $rt_dict__hash_key
    local.set $p2
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 1
    local.set $p3
    local.get $p3
    local.get $p2
    i32.const 0
    local.get $p1
    call $rt_dict__node_get
    ref.is_null
    i32.eqz
    local.set $p5
    local.get $p3
    local.get $p2
    i32.const 0
    local.get $p1
    call $rt_dict__node_remove
    local.set $p4
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 2
    local.set $p6
    local.get $p5
    (if
      (then
        local.get $p6
        ref.as_non_null
        local.get $p1
        call $rt_dict__order_remove_key
        local.set $p6))
    local.get $p0
    ref.as_non_null
    struct.get $rt_types__PDict 0
    local.get $p5
    i32.sub
    local.get $p4
    local.get $p6
    ref.as_non_null
    struct.new $rt_types__PDict
  )
  (func $rt_dict__set_in_place (type $functype_42)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (param $p2 anyref)
    (result (ref $rt_types__PDict))
    local.get $p0
    local.get $p1
    local.get $p2
    call $rt_dict__set
  )
  (func $rt_dict__remove_in_place (type $functype_43)
    (param $p0 (ref null $rt_types__PDict))
    (param $p1 anyref)
    (result (ref $rt_types__PDict))
    local.get $p0
    local.get $p1
    call $rt_dict__remove
  )
  (func $rt_core__print (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_print
  )
  (func $rt_core__println (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_println
  )
  (func $rt_core__eprint (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_eprint
  )
  (func $rt_core__eprintln (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_eprintln
  )
  (func $rt_core__trap (type $functype_1)
    (param $p0 (ref null $rt_types__String))
    local.get $p0
    call $rt_core__host_error
    unreachable
  )
  (func $rt_core__eq_array (type $functype_44)
    (param $p0 (ref null $rt_types__Array))
    (param $p1 (ref null $rt_types__Array))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    local.get $p0
    ref.is_null
    (if
      (then
        local.get $p1
        ref.is_null
        (if
          (then
            i32.const 1
            return))
        i32.const 0
        return))
    local.get $p1
    ref.is_null
    (if
      (then
        i32.const 0
        return))
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p2
    local.get $p1
    ref.as_non_null
    array.len
    local.get $p2
    i32.ne
    (if
      (then
        i32.const 0
        return))
    i32.const 0
    local.set $p3
    (block $exit
      (loop $loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Array
        local.get $p1
        ref.as_non_null
        local.get $p3
        array.get $rt_types__Array
        call $rt_core__eq
        i32.eqz
        (if
          (then
            i32.const 0
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $loop))
    i32.const 1
  )
  (func $rt_core__eq_vec (type $functype_45)
    (param $p0 (ref $rt_types__PVec))
    (param $p1 (ref $rt_types__PVec))
    (result i32)
    (local $p2 i32)
    (local $p3 i32)
    local.get $p0
    call $rt_arr__len
    local.set $p2
    local.get $p1
    call $rt_arr__len
    local.get $p2
    i32.ne
    (if
      (then
        i32.const 0
        return))
    i32.const 0
    local.set $p3
    (block $exit
      (loop $loop
        local.get $p3
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p0
        local.get $p3
        call $rt_arr__get
        local.get $p1
        local.get $p3
        call $rt_arr__get
        call $rt_core__eq
        i32.eqz
        (if
          (then
            i32.const 0
            return))
        local.get $p3
        i32.const 1
        i32.add
        local.set $p3
        br $loop))
    i32.const 1
  )
  (func $rt_core__eq_dict (type $functype_46)
    (param $p0 (ref $rt_types__PDict))
    (param $p1 (ref $rt_types__PDict))
    (result i32)
    (local $p2 i32)
    (local $p3 (ref $rt_types__PVec))
    (local $p4 i32)
    (local $p5 anyref)
    (local $p6 anyref)
    (local $p7 anyref)
    local.get $p0
    call $rt_dict__len
    local.set $p2
    local.get $p1
    call $rt_dict__len
    local.get $p2
    i32.ne
    (if
      (then
        i32.const 0
        return))
    local.get $p0
    call $rt_dict__keys
    local.set $p3
    i32.const 0
    local.set $p4
    (block $exit
      (loop $loop
        local.get $p4
        local.get $p2
        i32.ge_s
        br_if $exit
        local.get $p3
        local.get $p4
        call $rt_arr__get
        local.set $p5
        local.get $p0
        local.get $p5
        call $rt_dict__get
        local.set $p6
        local.get $p1
        local.get $p5
        call $rt_dict__get
        local.set $p7
        local.get $p7
        ref.is_null
        (if
          (then
            i32.const 0
            return))
        local.get $p6
        local.get $p7
        call $rt_core__eq
        i32.eqz
        (if
          (then
            i32.const 0
            return))
        local.get $p4
        i32.const 1
        i32.add
        local.set $p4
        br $loop))
    i32.const 1
  )
  (func $rt_core__eq_variant (type $functype_47)
    (param $p0 (ref $rt_types__Variant))
    (param $p1 (ref $rt_types__Variant))
    (result i32)
    (local $p2 (ref null $rt_types__Array))
    (local $p3 (ref null $rt_types__Array))
    local.get $p0
    struct.get $rt_types__Variant 0
    local.get $p1
    struct.get $rt_types__Variant 0
    i32.ne
    (if
      (then
        i32.const 0
        return))
    local.get $p0
    struct.get $rt_types__Variant 1
    local.get $p1
    struct.get $rt_types__Variant 1
    i32.ne
    (if
      (then
        i32.const 0
        return))
    local.get $p0
    struct.get $rt_types__Variant 2
    local.set $p2
    local.get $p1
    struct.get $rt_types__Variant 2
    local.set $p3
    local.get $p2
    local.get $p3
    call $rt_core__eq_array
  )
  (func $rt_core__eq (type $functype_48)
    (param $p0 anyref)
    (param $p1 anyref)
    (result i32)
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
    (if
      (then
        i32.const 1
        return))
    local.get $p0
    ref.test (ref i31)
    local.get $p1
    ref.test (ref i31)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref i31)
        i31.get_u
        local.get $p1
        ref.cast (ref i31)
        i31.get_u
        i32.eq
        return))
    local.get $p0
    ref.test (ref $rt_types__BoxedInt)
    local.get $p1
    ref.test (ref $rt_types__BoxedInt)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        local.get $p1
        ref.cast (ref $rt_types__BoxedInt)
        struct.get $rt_types__BoxedInt 0
        i64.eq
        return))
    local.get $p0
    ref.test (ref $rt_types__BoxedFloat)
    local.get $p1
    ref.test (ref $rt_types__BoxedFloat)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__BoxedFloat)
        struct.get $rt_types__BoxedFloat 0
        local.get $p1
        ref.cast (ref $rt_types__BoxedFloat)
        struct.get $rt_types__BoxedFloat 0
        f64.eq
        return))
    local.get $p0
    ref.test (ref $rt_types__String)
    local.get $p1
    ref.test (ref $rt_types__String)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__String)
        local.get $p1
        ref.cast (ref $rt_types__String)
        call $rt_str__eq
        return))
    local.get $p0
    ref.test (ref $rt_types__PVec)
    local.get $p1
    ref.test (ref $rt_types__PVec)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__PVec)
        local.get $p1
        ref.cast (ref $rt_types__PVec)
        call $rt_core__eq_vec
        return))
    local.get $p0
    ref.test (ref $rt_types__PDict)
    local.get $p1
    ref.test (ref $rt_types__PDict)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__PDict)
        local.get $p1
        ref.cast (ref $rt_types__PDict)
        call $rt_core__eq_dict
        return))
    local.get $p0
    ref.test (ref $rt_types__Variant)
    local.get $p1
    ref.test (ref $rt_types__Variant)
    i32.and
    (if
      (then
        local.get $p0
        ref.cast (ref $rt_types__Variant)
        local.get $p1
        ref.cast (ref $rt_types__Variant)
        call $rt_core__eq_variant
        return))
    local.get $p0
    ref.cast (ref null eq)
    local.get $p1
    ref.cast (ref null eq)
    ref.eq
  )
  (func $user__func_85 (type $functype_49)
    (param $p0 (ref null $rt_types__PVec))
    (param $p1 (ref null $rt_types__String))
    (result (ref null $rt_types__String))
    (local $p2 i64)
    (local $p3 i64)
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    (local $p7 (ref null $rt_types__String))
    (local $p8 i32)
    (local $p9 anyref)
    (local $p10 (ref null $rt_types__PVec))
    (local $p11 (ref null $rt_types__PVec))
    (local $p12 anyref)
    (local $p13 i64)
    (local $p14 anyref)
    (local $p15 i32)
    (local $p16 i32)
    (local $p17 i32)
    (local $p18 (ref null $rt_types__PVec))
    (local $p19 i64)
    (local $p20 i64)
    (local $p21 i64)
    (local $p22 i32)
    (local $p23 i32)
    (local $p24 i32)
    (local $p25 i64)
    (local $p26 i32)
    (local $p27 i32)
    (local $p28 i32)
    (local $p29 i32)
    (local $p30 i32)
    (local $p31 i32)
    (local $p32 (ref null $rt_types__String))
    (local $p33 anyref)
    (local $p34 (ref null $rt_types__PVec))
    (local $p35 (ref null $rt_types__PVec))
    (local $p36 i64)
    (local $p37 i64)
    (local $p38 i64)
    (local $p39 i32)
    (local $p40 i32)
    (local $p41 i32)
    (local $p42 i64)
    (local $p43 i32)
    (local $p44 i32)
    (local $p45 i32)
    (local $p46 i32)
    (local $p47 i32)
    (local $p48 i64)
    (local $p49 i32)
    (local $p50 i32)
    (local $p51 i32)
    (local $p52 (ref null $rt_types__PVec))
    (local $p53 i32)
    (local $p54 anyref)
    (local $p55 (ref null $rt_types__String))
    (local $p56 i32)
    (local $p57 (ref null $rt_types__String))
    local.get $p0
    call $rt_arr__len
    i64.extend_i32_s
    local.set $p2
    local.get $p2
    local.set $p3
    local.get $p3
    i64.const 0
    i64.eq
    local.set $p4
    local.get $p4
    (if (result i32)
      (then
        call $user____str_lit_get_empty
        return)
      (else
        i32.const 0))
    local.set $p5
    local.get $p3
    i64.const 1
    i64.eq
    local.set $p6
    local.get $p6
    (if (result i32)
      (then
        local.get $p0
        i64.const 0
        i32.wrap_i64
        call $rt_arr__get
        ref.cast (ref null $rt_types__String)
        local.set $p7
        local.get $p7
        return)
      (else
        i32.const 0))
    local.set $p8
    local.get $p1
    call $user__$string_utf8_bytes_helper
    call $rt_arr__from_array
    local.set $p9
    local.get $p9
    ref.cast (ref null $rt_types__PVec)
    local.set $p10
    global.get $rt_arr__empty_pvec
    local.set $p11
    local.get $p11
    local.set $p12
    i64.const 0
    local.set $p13
    call $rt_arr__builder_new
    local.set $p14
    (block $break_0 (result i32)
      (loop $cont_0
        local.get $p13
        local.get $p3
        i64.lt_s
        local.set $p15
        local.get $p15
        i32.eqz
        local.set $p16
        local.get $p16
        (if (result i32)
          (then
            i32.const 0
            br $break_0)
          (else
            local.get $p13
            i64.const 0
            i64.gt_s
            local.set $p17
            local.get $p17
            (if (result i32)
              (then
                local.get $p10
                local.set $p18
                local.get $p18
                call $rt_arr__len
                i64.extend_i32_s
                local.set $p19
                local.get $p19
                local.set $p20
                i64.const 0
                local.set $p21
                (block $break_1 (result i32)
                  (loop $cont_1
                    local.get $p21
                    local.get $p20
                    i64.ge_s
                    local.set $p22
                    local.get $p22
                    (if
                      (then
                        i32.const 0
                        br $break_1)
                      (else
                        local.get $p18
                        local.get $p21
                        i32.wrap_i64
                        call $rt_arr__get
                        ref.cast (ref i31)
                        i31.get_s
                        local.set $p23
                        local.get $p23
                        local.set $p24
                        local.get $p21
                        i64.const 1
                        i64.add
                        local.set $p25
                        local.get $p25
                        local.set $p21
                        i32.const 0
                        local.set $p26
                        local.get $p14
                        ref.cast (ref null $rt_types__Array)
                        local.get $p24
                        ref.i31
                        call $rt_arr__builder_push
                        i32.const 0
                        local.set $p27
                        i32.const 0
                        local.set $p28
                        br $cont_1))
                    local.get $p29
                    drop
                    br $cont_1)
                  unreachable)
                local.set $p30
                i32.const 0)
              (else
                i32.const 0))
            local.set $p31
            local.get $p0
            local.get $p13
            i32.wrap_i64
            call $rt_arr__get
            ref.cast (ref null $rt_types__String)
            local.set $p32
            local.get $p32
            call $user__$string_utf8_bytes_helper
            call $rt_arr__from_array
            local.set $p33
            local.get $p33
            ref.cast (ref null $rt_types__PVec)
            local.set $p34
            local.get $p34
            local.set $p35
            local.get $p35
            call $rt_arr__len
            i64.extend_i32_s
            local.set $p36
            local.get $p36
            local.set $p37
            i64.const 0
            local.set $p38
            (block $break_2 (result i32)
              (loop $cont_2
                local.get $p38
                local.get $p37
                i64.ge_s
                local.set $p39
                local.get $p39
                (if
                  (then
                    i32.const 0
                    br $break_2)
                  (else
                    local.get $p35
                    local.get $p38
                    i32.wrap_i64
                    call $rt_arr__get
                    ref.cast (ref i31)
                    i31.get_s
                    local.set $p40
                    local.get $p40
                    local.set $p41
                    local.get $p38
                    i64.const 1
                    i64.add
                    local.set $p42
                    local.get $p42
                    local.set $p38
                    i32.const 0
                    local.set $p43
                    local.get $p14
                    ref.cast (ref null $rt_types__Array)
                    local.get $p41
                    ref.i31
                    call $rt_arr__builder_push
                    i32.const 0
                    local.set $p44
                    i32.const 0
                    local.set $p45
                    br $cont_2))
                local.get $p46
                drop
                br $cont_2)
              unreachable)
            local.set $p47
            local.get $p13
            i64.const 1
            i64.add
            local.set $p48
            local.get $p48
            local.set $p13
            i32.const 0
            local.set $p49
            i32.const 0))
        local.set $p50
        local.get $p50
        drop
        br $cont_0)
      unreachable)
    local.set $p51
    local.get $p14
    ref.cast (ref null $rt_types__Array)
    call $rt_arr__builder_freeze
    local.set $p52
    local.get $p52
    local.set $p12
    i32.const 0
    local.set $p53
    local.get $p12
    ref.cast (ref null $rt_types__PVec)
    call $rt_arr__to_array
    call $user__$string_from_utf8_helper
    local.set $p54
    local.get $p54
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p54
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p54
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p54
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 0
    i32.const 0
    i32.eq
    local.get $p54
    ref.test (ref null $rt_types__Variant)
    (if (result (ref null $rt_types__Variant))
      (then
        local.get $p54
        ref.cast (ref null $rt_types__Variant))
      (else
        i32.const 0
        local.get $p54
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        local.get $p54
        ref.cast (ref null $user__option__String)
        struct.get $user__option__String 0
        i32.const 1
        i32.eq
        (if (result (ref $rt_types__Array))
          (then
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 1
            array.new_fixed $rt_types__Array 1)
          (else
            array.new_fixed $rt_types__Array 0))
        struct.new $rt_types__Variant))
    ref.cast (ref null $rt_types__Variant)
    struct.get $rt_types__Variant 1
    i32.const 1
    i32.eq
    i32.and
    (if (result (ref null $rt_types__String))
      (then
        local.get $p54
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p54
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p54
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 2
        i32.const 0
        array.get $rt_types__Array
        ref.cast (ref null $rt_types__String)
        local.set $p55
        local.get $p55)
      (else
        local.get $p54
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p54
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p54
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 0
        i32.const 0
        i32.eq
        local.get $p54
        ref.test (ref null $rt_types__Variant)
        (if (result (ref null $rt_types__Variant))
          (then
            local.get $p54
            ref.cast (ref null $rt_types__Variant))
          (else
            i32.const 0
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            local.get $p54
            ref.cast (ref null $user__option__String)
            struct.get $user__option__String 0
            i32.const 1
            i32.eq
            (if (result (ref $rt_types__Array))
              (then
                local.get $p54
                ref.cast (ref null $user__option__String)
                struct.get $user__option__String 1
                array.new_fixed $rt_types__Array 1)
              (else
                array.new_fixed $rt_types__Array 0))
            struct.new $rt_types__Variant))
        ref.cast (ref null $rt_types__Variant)
        struct.get $rt_types__Variant 1
        i32.const 0
        i32.eq
        i32.and
        (if (result (ref null $rt_types__String))
          (then
            call $user____str_lit_get_6a6f696e3a20696e76616c69642075746638
            call $rt_core__trap
            i32.const 0
            local.set $p56
            unreachable)
          (else
            call $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e205f5f7072656c7564655f766563746f722e6a6f696e202846756e6349642838352929
            call $rt_core__trap
            unreachable
            unreachable))))
    local.set $p57
    local.get $p57
    return
  )
  (func $user__func_88 (type $functype_50)
    (param $p0 (ref null $user__UserRecord_11))
    (result i64)
    (local $p1 i64)
    (local $p2 i64)
    (local $p3 i64)
    (local $p4 i64)
    (local $p5 i64)
    (local $p6 i64)
    (local $p7 i64)
    local.get $p0
    struct.get $user__UserRecord_11 0
    local.set $p1
    local.get $p0
    struct.get $user__UserRecord_11 0
    local.set $p2
    local.get $p1
    local.get $p2
    i64.mul
    local.set $p3
    local.get $p0
    struct.get $user__UserRecord_11 1
    local.set $p4
    local.get $p0
    struct.get $user__UserRecord_11 1
    local.set $p5
    local.get $p4
    local.get $p5
    i64.mul
    local.set $p6
    local.get $p3
    local.get $p6
    i64.add
    local.set $p7
    local.get $p7
    return
  )
  (func $user__func_89 (type $functype_51)
    (param $p0 (ref null $user__UserRecord_11))
    (param $p1 i64)
    (param $p2 i64)
    (result (ref null $user__UserRecord_11))
    (local $p3 i64)
    (local $p4 i64)
    (local $p5 i64)
    (local $p6 i64)
    (local $p7 (ref null $user__UserRecord_11))
    local.get $p0
    struct.get $user__UserRecord_11 0
    local.set $p3
    local.get $p3
    local.get $p1
    i64.add
    local.set $p4
    local.get $p0
    struct.get $user__UserRecord_11 1
    local.set $p5
    local.get $p5
    local.get $p2
    i64.add
    local.set $p6
    local.get $p4
    local.get $p6
    struct.new $user__UserRecord_11
    local.set $p7
    local.get $p7
    return
  )
  (func $user__func_90 (type $functype_52)
    (local $p0 (ref null $user__UserRecord_11))
    (local $p1 anyref)
    (local $p2 i64)
    (local $p3 (ref $rt_types__String))
    (local $p4 (ref $rt_types__String))
    (local $p5 i32)
    (local $p6 i64)
    (local $p7 (ref $rt_types__String))
    (local $p8 (ref $rt_types__String))
    (local $p9 i32)
    (local $p10 i64)
    (local $p11 (ref $rt_types__String))
    (local $p12 (ref $rt_types__String))
    (local $p13 i32)
    (local $p14 (ref null $user__UserRecord_11))
    (local $p15 i32)
    (local $p16 i64)
    (local $p17 (ref $rt_types__String))
    (local $p18 (ref $rt_types__String))
    (local $p19 i32)
    (local $p20 (ref null $user__UserRecord_11))
    (local $p21 (ref null $user__UserRecord_11))
    (local $p22 i64)
    (local $p23 (ref $rt_types__String))
    (local $p24 (ref $rt_types__String))
    (local $p25 i32)
    (local $p26 i64)
    (local $p27 (ref $rt_types__String))
    (local $p28 (ref $rt_types__String))
    (local $p29 i32)
    i64.const 3
    i64.const 4
    struct.new $user__UserRecord_11
    local.set $p0
    local.get $p0
    local.set $p1
    local.get $p1
    ref.cast (ref null $user__UserRecord_11)
    struct.get $user__UserRecord_11 0
    local.set $p2
    local.get $p2
    call $rt_str__from_i64
    local.set $p3
    call $user____str_lit_get_empty
    local.get $p3
    call $rt_str__concat
    local.set $p4
    local.get $p4
    call $rt_core__println
    i32.const 0
    local.set $p5
    local.get $p1
    ref.cast (ref null $user__UserRecord_11)
    struct.get $user__UserRecord_11 1
    local.set $p6
    local.get $p6
    call $rt_str__from_i64
    local.set $p7
    call $user____str_lit_get_empty
    local.get $p7
    call $rt_str__concat
    local.set $p8
    local.get $p8
    call $rt_core__println
    i32.const 0
    local.set $p9
    local.get $p1
    ref.cast (ref null $user__UserRecord_11)
    call $user__func_88
    local.set $p10
    local.get $p10
    call $rt_str__from_i64
    local.set $p11
    call $user____str_lit_get_empty
    local.get $p11
    call $rt_str__concat
    local.set $p12
    local.get $p12
    call $rt_core__println
    i32.const 0
    local.set $p13
    i64.const 4
    local.get $p1
    ref.cast (ref null $user__UserRecord_11)
    struct.get $user__UserRecord_11 1
    struct.new $user__UserRecord_11
    local.set $p14
    local.get $p14
    local.set $p1
    i32.const 0
    local.set $p15
    local.get $p1
    ref.cast (ref null $user__UserRecord_11)
    struct.get $user__UserRecord_11 0
    local.set $p16
    local.get $p16
    call $rt_str__from_i64
    local.set $p17
    call $user____str_lit_get_empty
    local.get $p17
    call $rt_str__concat
    local.set $p18
    local.get $p18
    call $rt_core__println
    i32.const 0
    local.set $p19
    local.get $p1
    ref.cast (ref null $user__UserRecord_11)
    i64.const 2
    i64.const 2
    call $user__func_89
    local.set $p20
    local.get $p20
    local.set $p21
    local.get $p21
    struct.get $user__UserRecord_11 0
    local.set $p22
    local.get $p22
    call $rt_str__from_i64
    local.set $p23
    call $user____str_lit_get_empty
    local.get $p23
    call $rt_str__concat
    local.set $p24
    local.get $p24
    call $rt_core__println
    i32.const 0
    local.set $p25
    local.get $p21
    call $user__func_88
    local.set $p26
    local.get $p26
    call $rt_str__from_i64
    local.set $p27
    call $user____str_lit_get_empty
    local.get $p27
    call $rt_str__concat
    local.set $p28
    local.get $p28
    return_call $rt_core__println
  )
  (func $user__func_85__closure (type $functype_53)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__PVec)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref null $rt_types__String)
    call $user__func_85
  )
  (func $user__func_88__closure (type $functype_53)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $user__UserRecord_11)
    call $user__func_88
    struct.new $rt_types__BoxedInt
  )
  (func $user__func_89__closure (type $functype_53)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 0
    array.get $rt_types__Array
    ref.cast (ref null $user__UserRecord_11)
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 1
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    local.get $p1
    ref.cast (ref null $rt_types__Array)
    i32.const 2
    array.get $rt_types__Array
    ref.cast (ref $rt_types__BoxedInt)
    struct.get $rt_types__BoxedInt 0
    call $user__func_89
  )
  (func $user__func_90__closure (type $functype_53)
    (param $p0 anyref)
    (param $p1 anyref)
    (result anyref)
    call $user__func_90
    i32.const 0
    ref.i31
  )
  (func $user__user____iterator_next (type $functype_54)
    (param $p0 anyref)
    (result (ref null $rt_types__Variant))
    (local $p1 (ref null $rt_types__Variant))
    (local $p2 i32)
    (local $p3 anyref)
    (local $p4 (ref null $rt_types__IterState))
    local.get $p0
    ref.cast (ref null $rt_types__IterState)
    local.set $p4
    local.get $p4
    struct.get $rt_types__IterState 1
    ref.cast (ref $rt_types__Closure)
    struct.get $rt_types__Closure 1
    local.get $p4
    struct.get $rt_types__IterState 0
    array.new_fixed $rt_types__Array 1
    local.get $p4
    struct.get $rt_types__IterState 1
    ref.cast (ref $rt_types__Closure)
    struct.get $rt_types__Closure 0
    call_ref $rt_types__ClosureFunc
    ref.cast (ref $rt_types__Variant)
    local.set $p1
    local.get $p1
    struct.get $rt_types__Variant 1
    local.set $p2
    local.get $p2
    i32.eqz
    (if (result (ref null $rt_types__Variant))
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p1
        struct.get $rt_types__Variant 2
        local.set $p3
        local.get $p3
        ref.cast (ref null $rt_types__Array)
        i32.const 0
        array.get $rt_types__Array
        local.get $p3
        ref.cast (ref null $rt_types__Array)
        i32.const 1
        array.get $rt_types__Array
        local.get $p4
        struct.get $rt_types__IterState 1
        struct.new $rt_types__IterState
        struct.new $user__UserRecord_5
        local.set $p3
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
    return
  )
  (func $user__$int_from_string_helper (type $functype_55)
    (param $p0 (ref null $rt_types__String))
    (result anyref)
    (local $p1 i64)
    (local $p2 i32)
    (local $p3 i32)
    (local $p4 i64)
    (local $p5 i32)
    (local $p6 i32)
    i64.const 1
    local.set $p4
    i32.const 1
    local.set $p6
    local.get $p0
    ref.as_non_null
    array.len
    local.set $p3
    local.get $p3
    i32.eqz
    (if
      (then
        i32.const 0
        local.set $p6)
      (else
        local.get $p0
        ref.as_non_null
        i32.const 0
        array.get_u $rt_types__String
        local.set $p5
        local.get $p5
        i32.const 45
        i32.eq
        (if
          (then
            i64.const -1
            local.set $p4
            i32.const 1
            local.set $p2
            local.get $p3
            i32.const 1
            i32.eq
            (if
              (then
                i32.const 0
                local.set $p6)))
          (else
            local.get $p5
            i32.const 43
            i32.eq
            (if
              (then
                i32.const 1
                local.set $p2
                local.get $p3
                i32.const 1
                i32.eq
                (if
                  (then
                    i32.const 0
                    local.set $p6)))
              (else
                i32.const 0
                local.set $p2))))
        local.get $p6
        (if
          (then
            (block $$done
              (loop $$digit_loop
                local.get $p2
                local.get $p3
                i32.ge_s
                br_if $$done
                local.get $p0
                ref.as_non_null
                local.get $p2
                array.get_u $rt_types__String
                local.set $p5
                local.get $p5
                i32.const 48
                i32.lt_s
                local.get $p5
                i32.const 57
                i32.gt_s
                i32.or
                (if
                  (then
                    i32.const 0
                    local.set $p6
                    br $$done))
                local.get $p1
                i64.const 10
                i64.mul
                local.get $p5
                i32.const 48
                i32.sub
                i64.extend_i32_u
                i64.add
                local.set $p1
                local.get $p2
                i32.const 1
                i32.add
                local.set $p2
                br $$digit_loop))))))
    local.get $p6
    (if (result anyref)
      (then
        i32.const 0
        i32.const 1
        local.get $p1
        local.get $p4
        i64.mul
        struct.new $rt_types__BoxedInt
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant)
      (else
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant))
  )
  (func $user__$from_code_point_helper (type $functype_56)
    (param $p0 i32)
    (result anyref)
    local.get $p0
    i32.const 0
    i32.lt_s
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p0
        i32.const 128
        i32.lt_u
        (if (result anyref)
          (then
            i32.const 0
            i32.const 1
            local.get $p0
            array.new_fixed $rt_types__String 1
            array.new_fixed $rt_types__Array 1
            struct.new $rt_types__Variant)
          (else
            local.get $p0
            i32.const 2048
            i32.lt_u
            (if (result anyref)
              (then
                i32.const 0
                i32.const 1
                local.get $p0
                i32.const 6
                i32.shr_u
                i32.const 192
                i32.or
                local.get $p0
                i32.const 63
                i32.and
                i32.const 128
                i32.or
                array.new_fixed $rt_types__String 2
                array.new_fixed $rt_types__Array 1
                struct.new $rt_types__Variant)
              (else
                local.get $p0
                i32.const 55296
                i32.ge_u
                local.get $p0
                i32.const 57343
                i32.le_u
                i32.and
                (if (result anyref)
                  (then
                    i32.const 0
                    i32.const 0
                    array.new_fixed $rt_types__Array 0
                    struct.new $rt_types__Variant)
                  (else
                    local.get $p0
                    i32.const 65535
                    i32.le_u
                    (if (result anyref)
                      (then
                        i32.const 0
                        i32.const 1
                        local.get $p0
                        i32.const 12
                        i32.shr_u
                        i32.const 224
                        i32.or
                        local.get $p0
                        i32.const 6
                        i32.shr_u
                        i32.const 63
                        i32.and
                        i32.const 128
                        i32.or
                        local.get $p0
                        i32.const 63
                        i32.and
                        i32.const 128
                        i32.or
                        array.new_fixed $rt_types__String 3
                        array.new_fixed $rt_types__Array 1
                        struct.new $rt_types__Variant)
                      (else
                        local.get $p0
                        i32.const 1114111
                        i32.le_u
                        (if (result anyref)
                          (then
                            i32.const 0
                            i32.const 1
                            local.get $p0
                            i32.const 18
                            i32.shr_u
                            i32.const 240
                            i32.or
                            local.get $p0
                            i32.const 12
                            i32.shr_u
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            local.get $p0
                            i32.const 6
                            i32.shr_u
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            local.get $p0
                            i32.const 63
                            i32.and
                            i32.const 128
                            i32.or
                            array.new_fixed $rt_types__String 4
                            array.new_fixed $rt_types__Array 1
                            struct.new $rt_types__Variant)
                          (else
                            i32.const 0
                            i32.const 0
                            array.new_fixed $rt_types__Array 0
                            struct.new $rt_types__Variant))))))))))))
  )
  (func $user__$string_utf8_bytes_helper (type $functype_57)
    (param $p0 (ref null $rt_types__String))
    (result (ref $rt_types__Array))
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref $rt_types__Array))
    local.get $p0
    array.len
    local.set $p1
    local.get $p1
    array.new_default $rt_types__Array
    local.set $p3
    i32.const 0
    local.set $p2
    (block $$break
      (loop $$continue
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $$break
        local.get $p3
        local.get $p2
        local.get $p0
        local.get $p2
        array.get_u $rt_types__String
        ref.i31
        array.set $rt_types__Array
        local.get $p2
        i32.const 1
        i32.add
        local.set $p2
        br $$continue))
    local.get $p3
  )
  (func $user__$string_from_utf8_helper (type $functype_58)
    (param $p0 (ref null $rt_types__Array))
    (result anyref)
    (local $p1 i32)
    (local $p2 i32)
    (local $p3 (ref null $rt_types__String))
    (local $p4 i32)
    (local $p5 i32)
    (local $p6 i32)
    local.get $p0
    array.len
    local.set $p1
    i32.const 1
    local.set $p5
    i32.const 0
    local.set $p2
    (block $$vbreak
      (loop $$vcont
        local.get $p2
        local.get $p1
        i32.ge_u
        br_if $$vbreak
        local.get $p0
        local.get $p2
        array.get $rt_types__Array
        ref.cast (ref i31)
        i31.get_u
        local.set $p4
        local.get $p4
        i32.const 128
        i32.lt_u
        (if
          (then
            local.get $p2
            i32.const 1
            i32.add
            local.set $p2)
          (else
            local.get $p4
            i32.const 192
            i32.ge_u
            local.get $p4
            i32.const 223
            i32.le_u
            i32.and
            (if
              (then
                local.get $p4
                i32.const 194
                i32.lt_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p2
                i32.const 1
                i32.add
                local.get $p1
                i32.ge_u
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p0
                local.get $p2
                i32.const 1
                i32.add
                array.get $rt_types__Array
                ref.cast (ref i31)
                i31.get_u
                local.set $p6
                local.get $p6
                i32.const 192
                i32.and
                i32.const 128
                i32.ne
                (if
                  (then
                    i32.const 0
                    local.set $p5
                    br $$vbreak))
                local.get $p2
                i32.const 2
                i32.add
                local.set $p2)
              (else
                local.get $p4
                i32.const 224
                i32.ge_u
                local.get $p4
                i32.const 239
                i32.le_u
                i32.and
                (if
                  (then
                    local.get $p2
                    i32.const 2
                    i32.add
                    local.get $p1
                    i32.ge_u
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p0
                    local.get $p2
                    i32.const 1
                    i32.add
                    array.get $rt_types__Array
                    ref.cast (ref i31)
                    i31.get_u
                    i32.const 192
                    i32.and
                    i32.const 128
                    i32.ne
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p0
                    local.get $p2
                    i32.const 2
                    i32.add
                    array.get $rt_types__Array
                    ref.cast (ref i31)
                    i31.get_u
                    i32.const 192
                    i32.and
                    i32.const 128
                    i32.ne
                    (if
                      (then
                        i32.const 0
                        local.set $p5
                        br $$vbreak))
                    local.get $p4
                    i32.const 224
                    i32.eq
                    (if
                      (then
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 160
                        i32.lt_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))))
                    local.get $p4
                    i32.const 237
                    i32.eq
                    (if
                      (then
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 160
                        i32.ge_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))))
                    local.get $p2
                    i32.const 3
                    i32.add
                    local.set $p2)
                  (else
                    local.get $p4
                    i32.const 240
                    i32.ge_u
                    local.get $p4
                    i32.const 244
                    i32.le_u
                    i32.and
                    (if
                      (then
                        local.get $p2
                        i32.const 3
                        i32.add
                        local.get $p1
                        i32.ge_u
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 1
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 2
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p0
                        local.get $p2
                        i32.const 3
                        i32.add
                        array.get $rt_types__Array
                        ref.cast (ref i31)
                        i31.get_u
                        i32.const 192
                        i32.and
                        i32.const 128
                        i32.ne
                        (if
                          (then
                            i32.const 0
                            local.set $p5
                            br $$vbreak))
                        local.get $p4
                        i32.const 240
                        i32.eq
                        (if
                          (then
                            local.get $p0
                            local.get $p2
                            i32.const 1
                            i32.add
                            array.get $rt_types__Array
                            ref.cast (ref i31)
                            i31.get_u
                            i32.const 144
                            i32.lt_u
                            (if
                              (then
                                i32.const 0
                                local.set $p5
                                br $$vbreak))))
                        local.get $p4
                        i32.const 244
                        i32.eq
                        (if
                          (then
                            local.get $p0
                            local.get $p2
                            i32.const 1
                            i32.add
                            array.get $rt_types__Array
                            ref.cast (ref i31)
                            i31.get_u
                            i32.const 144
                            i32.ge_u
                            (if
                              (then
                                i32.const 0
                                local.set $p5
                                br $$vbreak))))
                        local.get $p2
                        i32.const 4
                        i32.add
                        local.set $p2)
                      (else
                        i32.const 0
                        local.set $p5
                        br $$vbreak))))))))
        br $$vcont))
    local.get $p5
    i32.eqz
    (if (result anyref)
      (then
        i32.const 0
        i32.const 0
        array.new_fixed $rt_types__Array 0
        struct.new $rt_types__Variant)
      (else
        local.get $p1
        array.new_default $rt_types__String
        local.set $p3
        i32.const 0
        local.set $p2
        (block $$cbreak
          (loop $$ccont
            local.get $p2
            local.get $p1
            i32.ge_u
            br_if $$cbreak
            local.get $p3
            local.get $p2
            local.get $p0
            local.get $p2
            array.get $rt_types__Array
            ref.cast (ref i31)
            i31.get_u
            array.set $rt_types__String
            local.get $p2
            i32.const 1
            i32.add
            local.set $p2
            br $$ccont))
        i32.const 0
        i32.const 1
        local.get $p3
        array.new_fixed $rt_types__Array 1
        struct.new $rt_types__Variant))
  )
  (func $user____user_init (type $functype_52)
    call $user__func_90
  )
  (func $user____str_lit_get_empty (type $functype_59)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_empty
    ref.is_null
    (if
      (then
        array.new_fixed $rt_types__String 0
        global.set $user____str_lit_global_empty))
    global.get $user____str_lit_global_empty
    ref.as_non_null
  )
  (func $user____str_lit_get_6a6f696e3a20696e76616c69642075746638 (type $functype_59)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6a6f696e3a20696e76616c69642075746638
    ref.is_null
    (if
      (then
        i32.const 106
        i32.const 111
        i32.const 105
        i32.const 110
        i32.const 58
        i32.const 32
        i32.const 105
        i32.const 110
        i32.const 118
        i32.const 97
        i32.const 108
        i32.const 105
        i32.const 100
        i32.const 32
        i32.const 117
        i32.const 116
        i32.const 102
        i32.const 56
        array.new_fixed $rt_types__String 18
        global.set $user____str_lit_global_6a6f696e3a20696e76616c69642075746638))
    global.get $user____str_lit_global_6a6f696e3a20696e76616c69642075746638
    ref.as_non_null
  )
  (func $user____str_lit_get_6e6f6e2d65786861757374697665206d6174636820696e205f5f7072656c7564655f766563746f722e6a6f696e202846756e6349642838352929 (type $functype_59)
    (result (ref $rt_types__String))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f7072656c7564655f766563746f722e6a6f696e202846756e6349642838352929
    ref.is_null
    (if
      (then
        i32.const 110
        i32.const 111
        i32.const 110
        i32.const 45
        i32.const 101
        i32.const 120
        i32.const 104
        i32.const 97
        i32.const 117
        i32.const 115
        i32.const 116
        i32.const 105
        i32.const 118
        i32.const 101
        i32.const 32
        i32.const 109
        i32.const 97
        i32.const 116
        i32.const 99
        i32.const 104
        i32.const 32
        i32.const 105
        i32.const 110
        i32.const 32
        i32.const 95
        i32.const 95
        i32.const 112
        i32.const 114
        i32.const 101
        i32.const 108
        i32.const 117
        i32.const 100
        i32.const 101
        i32.const 95
        i32.const 118
        i32.const 101
        i32.const 99
        i32.const 116
        i32.const 111
        i32.const 114
        i32.const 46
        i32.const 106
        i32.const 111
        i32.const 105
        i32.const 110
        i32.const 32
        i32.const 40
        i32.const 70
        i32.const 117
        i32.const 110
        i32.const 99
        i32.const 73
        i32.const 100
        i32.const 40
        i32.const 56
        i32.const 53
        i32.const 41
        i32.const 41
        array.new_fixed $rt_types__String 58
        global.set $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f7072656c7564655f766563746f722e6a6f696e202846756e6349642838352929))
    global.get $user____str_lit_global_6e6f6e2d65786861757374697665206d6174636820696e205f5f7072656c7564655f766563746f722e6a6f696e202846756e6349642838352929
    ref.as_non_null
  )
  (func $__linked_init (type $functype_52)
    call $user____user_init
  )
  (export "rt_arr__tailoff" (func $rt_arr__tailoff))
  (export "rt_arr__get_leaf" (func $rt_arr__get_leaf))
  (export "rt_arr__new_path" (func $rt_arr__new_path))
  (export "rt_arr__push_tail" (func $rt_arr__push_tail))
  (export "rt_arr__do_set" (func $rt_arr__do_set))
  (export "rt_arr__push" (func $rt_arr__push))
  (export "rt_arr__make" (func $rt_arr__make))
  (export "rt_arr__get" (func $rt_arr__get))
  (export "rt_arr__set" (func $rt_arr__set))
  (export "rt_arr__len" (func $rt_arr__len))
  (export "rt_arr__concat" (func $rt_arr__concat))
  (export "rt_arr__slice" (func $rt_arr__slice))
  (export "rt_arr__builder_new" (func $rt_arr__builder_new))
  (export "rt_arr__builder_from" (func $rt_arr__builder_from))
  (export "rt_arr__builder_push" (func $rt_arr__builder_push))
  (export "rt_arr__builder_extend" (func $rt_arr__builder_extend))
  (export "rt_arr__builder_freeze" (func $rt_arr__builder_freeze))
  (export "rt_arr__from_array" (func $rt_arr__from_array))
  (export "rt_arr__to_array" (func $rt_arr__to_array))
  (export "rt_arr__from_read_file_result" (func $rt_arr__from_read_file_result))
  (export "rt_str__len" (func $rt_str__len))
  (export "rt_str__concat" (func $rt_str__concat))
  (export "rt_str__substring" (func $rt_str__substring))
  (export "rt_str__eq" (func $rt_str__eq))
  (export "rt_str__cmp" (func $rt_str__cmp))
  (export "rt_str__from_i64" (func $rt_str__from_i64))
  (export "rt_str__from_f64" (func $rt_str__from_f64))
  (export "rt_str__from_bool" (func $rt_str__from_bool))
  (export "rt_dict__popcount" (func $rt_dict__popcount))
  (export "rt_dict__arr_insert_at" (func $rt_dict__arr_insert_at))
  (export "rt_dict__arr_replace_at" (func $rt_dict__arr_replace_at))
  (export "rt_dict__arr_remove_at" (func $rt_dict__arr_remove_at))
  (export "rt_dict__hash_i64" (func $rt_dict__hash_i64))
  (export "rt_dict__hash_string" (func $rt_dict__hash_string))
  (export "rt_dict__hash_key" (func $rt_dict__hash_key))
  (export "rt_dict__collision_get" (func $rt_dict__collision_get))
  (export "rt_dict__collision_set" (func $rt_dict__collision_set))
  (export "rt_dict__node_get" (func $rt_dict__node_get))
  (export "rt_dict__node_set" (func $rt_dict__node_set))
  (export "rt_dict__node_remove" (func $rt_dict__node_remove))
  (export "rt_dict__order_remove_key" (func $rt_dict__order_remove_key))
  (export "rt_dict__make" (func $rt_dict__make))
  (export "rt_dict__len" (func $rt_dict__len))
  (export "rt_dict__keys" (func $rt_dict__keys))
  (export "rt_dict__has" (func $rt_dict__has))
  (export "rt_dict__get" (func $rt_dict__get))
  (export "rt_dict__get_option" (func $rt_dict__get_option))
  (export "rt_dict__set" (func $rt_dict__set))
  (export "rt_dict__remove" (func $rt_dict__remove))
  (export "rt_dict__set_in_place" (func $rt_dict__set_in_place))
  (export "rt_dict__remove_in_place" (func $rt_dict__remove_in_place))
  (export "rt_core__print" (func $rt_core__print))
  (export "rt_core__println" (func $rt_core__println))
  (export "rt_core__eprint" (func $rt_core__eprint))
  (export "rt_core__eprintln" (func $rt_core__eprintln))
  (export "rt_core__trap" (func $rt_core__trap))
  (export "rt_core__eq_array" (func $rt_core__eq_array))
  (export "rt_core__eq_vec" (func $rt_core__eq_vec))
  (export "rt_core__eq_dict" (func $rt_core__eq_dict))
  (export "rt_core__eq_variant" (func $rt_core__eq_variant))
  (export "rt_core__eq" (func $rt_core__eq))
  (start $__linked_init)
)