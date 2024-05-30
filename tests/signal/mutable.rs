use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicUsize};
use std::task::Poll;
use futures_signals::cancelable_future;
use futures_signals::signal::{SignalExt, Mutable, channel, Memo, ReadOnlyMutable, Compute1, Compute2, Reader, BoxReader};
use crate::util;


#[test]
fn test_mutable() {
    let mutable = Mutable::new(1);
    let mut s1 = mutable.signal();
    let mut s2 = mutable.signal_cloned();

    util::with_noop_context(|cx| {
        assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
        assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(5);
        assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(5)));
        assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(5)));
        assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable);
        assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn test_mutable_drop() {
    {
        let mutable = Mutable::new(1);
        let mut s1 = mutable.signal();
        let mut s2 = mutable.signal_cloned();
        drop(mutable);

        util::with_noop_context(|cx| {
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));
        });
    }

    {
        let mutable = Mutable::new(1);
        let mut s1 = mutable.signal();
        let mut s2 = mutable.signal_cloned();

        util::with_noop_context(|cx| {
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

            mutable.set(5);
            drop(mutable);

            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));
        });
    }

    {
        let mutable = Mutable::new(1);
        let mut s1 = mutable.signal();
        let mut s2 = mutable.signal_cloned();

        util::with_noop_context(|cx| {
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(1)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Pending);

            mutable.set(5);
            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s1.poll_change_unpin(cx), Poll::Pending);

            drop(mutable);
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(Some(5)));
            assert_eq!(s2.poll_change_unpin(cx), Poll::Ready(None));

            assert_eq!(s1.poll_change_unpin(cx), Poll::Ready(None));
        });
    }
}

#[test]
fn test_send_sync() {
    let a = cancelable_future(async {}, || ());
    let _: Box<dyn Send + Sync> = Box::new(a.0);
    let _: Box<dyn Send + Sync> = Box::new(a.1);

    let _: Box<dyn Send + Sync> = Box::new(Mutable::new(1));
    let _: Box<dyn Send + Sync> = Box::new(Mutable::new(1).signal());
    let _: Box<dyn Send + Sync> = Box::new(Mutable::new(1).signal_cloned());

    let a = channel(1);
    let _: Box<dyn Send + Sync> = Box::new(a.0);
    let _: Box<dyn Send + Sync> = Box::new(a.1);
}

// Verifies that lock_mut only notifies when it is mutated
#[test]
fn test_lock_mut() {
    {
        let m = Mutable::new(1);

        let polls = util::get_signal_polls(m.signal(), move || {
            let mut lock = m.lock_mut();

            if *lock == 2 {
                *lock = 5;
            }
        });

        assert_eq!(polls, vec![
            Poll::Ready(Some(1)),
            Poll::Pending,
            Poll::Ready(None),
        ]);
    }

    {
        let m = Mutable::new(1);

        let polls = util::get_signal_polls(m.signal(), move || {
            let mut lock = m.lock_mut();

            if *lock == 1 {
                *lock = 5;
            }
        });

        assert_eq!(polls, vec![
            Poll::Ready(Some(1)),
            Poll::Pending,
            Poll::Ready(Some(5)),
            Poll::Ready(None),
        ]);
    }
}


/*#[test]
fn test_lock_panic() {
    struct Foo;

    impl Foo {
        fn bar<A>(self, _value: &A) -> Self {
            self
        }

        fn qux<A>(self, _value: A) -> Self {
            self
        }
    }

    let m = Mutable::new(1);

    Foo
        .bar(&m.lock_ref())
        .qux(m.signal().map(move |x| x * 10));
}


#[test]
fn test_lock_mut_signal() {
    let m = Mutable::new(1);

    let mut output = {
        let mut lock = m.lock_mut();
        let output = lock.signal().map(move |x| x * 10);
        *lock = 2;
        output
    };

    util::with_noop_context(|cx| {
        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(Some(20)));
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);

        m.set(5);

        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(Some(50)));
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(output.poll_change_unpin(cx), Poll::Pending);

        drop(m);

        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(output.poll_change_unpin(cx), Poll::Ready(None));
    });
}*/

#[test]
fn is_from_t() {
    let src = 0;
    let _out: Mutable<u8> = Mutable::from(src);

    let src = 0;
    let _out: Mutable<u8> = src.into();
}

#[test]
fn memo_reader_usize() {
    let mutable = Mutable::new(10);
    let memo = Memo::new(Compute1::new(mutable.read_only(), |m| m * 2));

    let mutable_i = mutable.read_only();
    immutable(mutable, mutable_i, memo);

    fn immutable<I1: Reader<Item=usize>, I2: Reader<Item=usize>>(mutable: Mutable<usize>, mutable_reader: I1, memo_reader: I2) {
        let memo2 = Memo::new(Compute1::new(memo_reader.clone(), |m| m * 2));

        assert_eq!(mutable_reader.get(), 10);
        assert_eq!(memo_reader.get(), 20);
        assert_eq!(memo2.get(), 40);

        let mut mutable_signal = mutable_reader.signal();
        let mut memo_signal = memo_reader.signal();

        util::with_noop_context(|cx| {
            assert_eq!(mutable_reader.get(), 10);
            assert_eq!(memo_reader.get(), 20);

            mutable.set(20);
            assert_eq!(mutable_reader.get(), 20);
            assert_eq!(memo_reader.get(), 40);

            assert_eq!(mutable_signal.poll_change_unpin(cx), Poll::Ready(Some(20)));
            assert_eq!(mutable_signal.poll_change_unpin(cx), Poll::Pending);
            assert_eq!(memo_signal.poll_change_unpin(cx), Poll::Ready(Some(40)));
            assert_eq!(memo_signal.poll_change_unpin(cx), Poll::Pending);

            mutable.set(30);

            // here we expect the signal to recompute the memo without depending on the memo to recompute itself
            assert_eq!(mutable_signal.poll_change_unpin(cx), Poll::Ready(Some(30)));
            assert_eq!(mutable_signal.poll_change_unpin(cx), Poll::Pending);
            assert_eq!(memo_signal.poll_change_unpin(cx), Poll::Ready(Some(60)));
            assert_eq!(memo_signal.poll_change_unpin(cx), Poll::Pending);
        });
    }
}

#[test]
fn memo_ensure_lazy_compute() {
    let mutable = Mutable::new(10);

    thread_local! {
        static MEMO_COUNTER: std::cell::Cell<usize> = std::cell::Cell::new(0);
    }

    MEMO_COUNTER.set(0);

    let memo = Memo::new(Compute1::new(mutable.read_only(), |_m| {
        let value = MEMO_COUNTER.get() + 1;
        MEMO_COUNTER.set(value);
        value
    }));

    mutable.set(20);
    mutable.set(30);

    assert_eq!(memo.get(), 1);
}

#[test]
fn memo_from_one_mutable_usize() {
    let mutable = Mutable::new(10);
    let memo = Memo::new(Compute1::new(mutable.read_only(), |m| m * 2));

    let mut memo_signal1 = memo.signal();
    let mut memo_signal2 = memo.signal();

    util::with_noop_context(|cx| {
        assert_eq!(memo.get(), 20);

        mutable.set(20);
        assert_eq!(memo.get(), 40);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(40)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(40)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(30);

        // here we expect the signal to recompute the memo without depending on the memo to recompute itself
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(60)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(60)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn broadcast_from_memo_from_one_mutable_usize() {
    let mutable = Mutable::new(10);
    let memo = Memo::new(Compute1::new(mutable.read_only(), |m| m * 2));

    let broadcaster = memo.signal().broadcast();

    let mut memo_signal1 = broadcaster.signal();
    let mut memo_signal2 = broadcaster.signal();

    util::with_noop_context(|cx| {
        assert_eq!(memo.get(), 20);

        mutable.set(20);
        assert_eq!(memo.get(), 40);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(40)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(40)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(30);

        // here we expect the signal to recompute the memo without depending on the memo to recompute itself
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(60)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(60)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn memo_from_one_mutable_string() {
    let mutable = Mutable::new("a".to_string());
    let memo = Memo::new(Compute1::new(mutable.read_only(), |m| m + "_suffix"));

    let mut memo_signal1 = memo.signal_cloned();
    let mut memo_signal2 = memo.signal_cloned();

    util::with_noop_context(|cx| {
        assert_eq!(memo.get_cloned(), "a_suffix");

        mutable.set("b".to_string());
        assert_eq!(memo.get_cloned(), "b_suffix");

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some("b_suffix".to_string())));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some("b_suffix".to_string())));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        mutable.set("c".to_string());

        // here we expect the signal to recompute the memo without depending on the memo to recompute itself
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some("c_suffix".to_string())));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some("c_suffix".to_string())));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn memo_from_two_mutables_usize() {
    let mutable1 = Mutable::new(1);
    let mutable2 = Mutable::new(2);
    let memo = Memo::new(Compute2::new(mutable1.read_only(), mutable2.read_only(), |m1, m2| m1 + m2));

    let mut memo_signal1 = memo.signal();
    let mut memo_signal2 = memo.signal();

    util::with_noop_context(|cx| {
        assert_eq!(memo.get(), 3);

        mutable1.set(10);
        assert_eq!(memo.get(), 12);
        mutable2.set(20);
        assert_eq!(memo.get(), 30);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(30)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(30)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        mutable1.set(100);
        assert_eq!(memo.get(), 120);
        mutable2.set(200);
        assert_eq!(memo.get(), 300);

        // here we expect the signal to recompute the memo without depending on the memo to recompute itself
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(300)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(300)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable1);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn memo_from_one_memo_usize() {
    let mutable = Mutable::new(10);
    let src_memo = Memo::new(Compute1::new(mutable.read_only(), |m| m * 2));
    let memo = Memo::new(Compute1::new(src_memo, |m| m * 2));

    let mut memo_signal1 = memo.signal();
    let mut memo_signal2 = memo.signal();

    util::with_noop_context(|cx| {
        assert_eq!(memo.get(), 40);

        mutable.set(20);
        assert_eq!(memo.get(), 80);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(80)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(80)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        mutable.set(30);

        // here we expect the signal to recompute the memo without depending on the memo to recompute itself
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(120)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(120)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn memo_from_two_memos_usize() {
    let mutable1 = Mutable::new(1);
    let src_memo1 = Memo::new(Compute1::new(mutable1.read_only(), |m| m * 10));
    let mutable2 = Mutable::new(2);
    let src_memo2 = Memo::new(Compute1::new(mutable2.read_only(), |m| m * 10));
    let memo = Memo::new(Compute2::new(src_memo1, src_memo2, |m1, m2| m1 + m2));

    let mut memo_signal1 = memo.signal();
    let mut memo_signal2 = memo.signal();

    util::with_noop_context(|cx| {
        assert_eq!(memo.get(), 30);

        mutable1.set(10);
        assert_eq!(memo.get(), 120);
        mutable2.set(20);
        assert_eq!(memo.get(), 300);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(300)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(300)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        mutable1.set(100);
        assert_eq!(memo.get(), 1200);
        mutable2.set(200);
        assert_eq!(memo.get(), 3000);

        // here we expect the signal to recompute the memo without depending on the memo to recompute itself
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(Some(3000)));
        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Pending);
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(Some(3000)));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Pending);

        drop(mutable1);

        assert_eq!(memo_signal1.poll_change_unpin(cx), Poll::Ready(None));
        assert_eq!(memo_signal2.poll_change_unpin(cx), Poll::Ready(None));
    });
}

#[test]
fn memos_in_struct() {
    let mutable = Mutable::new(10);
    let memo = Memo::new(Compute1::new(mutable.read_only(), |m| m * 2));

    struct MemoInStruct<MemoReader: Reader<Item=usize>> {
        memo: MemoReader,
        memo_memo: Memo<Compute1<usize, MemoReader>>,
    }

    impl<MemoReader: Reader<Item=usize>> MemoInStruct<MemoReader> {
        fn new1(memo: MemoReader) -> Self {
            let memo_memo = Memo::new(Compute1::new(memo.clone(), |m| m * 2));

            Self {
                memo,
                memo_memo,
            }
        }
    }

    let memo_in_struct = MemoInStruct::new1(memo);

    assert_eq!(memo_in_struct.memo.get(), 20);
    assert_eq!(memo_in_struct.memo_memo.get(), 40);

    mutable.set(11);
    assert_eq!(memo_in_struct.memo.get(), 22);
    assert_eq!(memo_in_struct.memo_memo.get(), 44);
}

#[test]
fn boxed_reader_usize() {
    let mutable = Mutable::new(10);

    let boxed_mutable = mutable.boxed();

    boxed_fn(&boxed_mutable, 10);

    mutable.set(20);
    boxed_fn(&boxed_mutable, 20);

    fn boxed_fn(r: &BoxReader<i32>, expected_value: i32) {
        let r1 = r.clone_reader();
        let v = r.get();
        let v1 = r1.get();

        assert_eq!(v, expected_value);
        assert_eq!(v1, expected_value);
    }
}

#[test]
fn boxed_reader_struct() {
    let mutable_usize = Mutable::new(10);

    #[derive(Clone)]
    struct SomeStruct {
        some_data: usize,
        #[allow(unused)]
        some_read_only_mutable: ReadOnlyMutable<i32>,
        #[allow(unused)]
        some_mutable: Mutable<i32>,
    }

    let mutable = Mutable::new(Arc::new(SomeStruct {
        some_data: 10,
        some_read_only_mutable: mutable_usize.read_only(),
        some_mutable: mutable_usize,
    }));

    let memo = Memo::new(Compute1::new(mutable.read_only(), |m| {
        let mut new_value = m.deref().clone();
        new_value.some_data = new_value.some_data + 20;
        Arc::new(new_value)
    }));

    boxed_fn(mutable.boxed(), 10);
    boxed_fn(memo.boxed(), 30);

    fn boxed_fn(r: BoxReader<Arc<SomeStruct>>, expect_value: usize) {
        let r1 = r.clone_reader();
        let v = r.get();
        let v1 = r1.get();

        assert_eq!(v.some_data, expect_value);
        assert_eq!(v1.some_data, expect_value);
    }
}

#[test]
fn memo_from_boxed() {
    let boxed_mutable = Mutable::new(10).boxed();
    assert_eq!(boxed_mutable.get(), 10);
    let memo = Memo::new(Compute1::new(boxed_mutable, |m| {
        m + 10
    }));
    let memo_boxed = memo.boxed();
    assert_eq!(memo_boxed.get(), 20);
}

#[test]
fn test_memo_thread() {
    #[derive(Clone)]
    struct MyStruct {
        a: usize,
        b: String,
    }

    let mutable_usize = Mutable::new(10);
    let mutable_struct = Mutable::new(MyStruct { a: 10, b: "asdf".to_string() });
    let boxed_mutable = Mutable::new(10).boxed();
    assert_eq!(boxed_mutable.get(), 10);
    let memo = Memo::new(Compute1::new(boxed_mutable.clone(), |m| {
        m + 10
    }));
    let memo_boxed = memo.boxed();
    assert_eq!(memo_boxed.get(), 20);

    use std::thread;

    let handle = thread::spawn(move || {
        assert_eq!(mutable_usize.get(), 10);
        assert_eq!(mutable_struct.get_cloned().a, 10);
        assert_eq!(boxed_mutable.get(), 10);
        assert_eq!(memo_boxed.get(), 20);
    });

    handle.join().unwrap(); // block main thread until new thread finishes
}
