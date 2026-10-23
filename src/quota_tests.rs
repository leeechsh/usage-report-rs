use super::*;

#[test]
fn enumeration_retains_rows_at_both_end_markers() {
    for errno in [libc::ENOENT, libc::ESRCH] {
        let rows = collect_quotas(true, 0, |cursor, info| {
            if cursor > 1004 {
                return Err(std::io::Error::from_raw_os_error(errno));
            }
            info.id = if cursor == 0 { 1000 } else { 1004 };
            info.hard = 100;
            Ok(())
        })
        .unwrap();
        assert_eq!(rows.iter().map(|q| q.uid).collect::<Vec<_>>(), [1000, 1004]);
        assert_eq!(rows[0].hard, 102400);
    }
}

#[test]
fn errors_and_non_advancing_ids_are_not_hidden() {
    assert!(read_quotas("/definitely-missing-usage-report-device").is_err());
    for errno in [libc::EIO, libc::EPERM] {
        assert!(
            collect_quotas(true, 0, |_, _| Err(std::io::Error::from_raw_os_error(
                errno
            )))
            .is_err()
        );
    }
    assert!(
        collect_quotas(false, 1004, |_, _| Err(std::io::Error::from_raw_os_error(
            libc::ENOENT
        )))
        .is_err()
    );
    assert!(
        collect_quotas(true, 0, |_, info| {
            info.id = 0;
            Ok(())
        })
        .is_err()
    );
    let rows = collect_quotas(true, 0, |_, info| {
        info.id = u32::MAX;
        info.hard = 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
}
