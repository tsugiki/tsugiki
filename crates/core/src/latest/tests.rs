use super::*;

#[test]
fn a_reader_starts_on_the_first_value() {
    let (_writer, reader) = channel(1);
    assert_eq!(reader.get(), 1);
}

#[test]
fn every_reader_sees_what_the_writer_set_last() {
    let (mut writer, reader) = channel(1);
    let another = reader.clone();

    writer.set(&2);

    assert_eq!(reader.get(), 2);
    assert_eq!(another.get(), 2);
}

#[test]
fn a_reader_keeps_the_last_value_after_the_writer_has_gone() {
    let (mut writer, reader) = channel(1);
    writer.set(&2);

    drop(writer);

    assert_eq!(reader.get(), 2);
}

#[test]
fn a_panic_while_holding_the_lock_doesnt_stop_it_working() {
    let (mut writer, reader) = channel(1);
    std::thread::scope(|scope| {
        let holder = scope.spawn(|| {
            let _guard = reader.0.lock();
            panic!("a panic while holding the lock");
        });
        assert!(holder.join().is_err());
    });
    assert!(reader.0.is_poisoned());

    assert_eq!(reader.get(), 1);
    writer.set(&2);
    assert_eq!(reader.get(), 2);
}
