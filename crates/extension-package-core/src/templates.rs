use super::*;

pub(crate) fn template_text(path: &'static str) -> Result<&'static str, ExtensionPackageError> {
    PACKAGE_TEMPLATE_DIR
        .get_file(path)
        .context(TemplateMissingSnafu { path })?
        .contents_utf8()
        .context(TemplateUtf8Snafu { path })
}

pub(crate) fn template_bytes(path: &'static str) -> Result<&'static [u8], ExtensionPackageError> {
    Ok(PACKAGE_TEMPLATE_DIR
        .get_file(path)
        .context(TemplateMissingSnafu { path })?
        .contents())
}

pub(crate) fn text_file(path: &str, content: &str) -> PackageFile {
    PackageFile {
        path: path.to_string(),
        content: content.as_bytes().to_vec(),
    }
}

pub(crate) fn yaml_file<T>(path: &str, value: &T) -> Result<PackageFile, ExtensionPackageError>
where
    T: Serialize,
{
    let content = serde_yaml::to_string(value).context(SerializeSnafu { name: "yaml file" })?;
    Ok(PackageFile {
        path: path.to_string(),
        content: content.into_bytes(),
    })
}

pub(crate) fn package_file_meta(files: &[PackageFile]) -> Vec<PackageFileMeta> {
    files
        .iter()
        .map(|file| PackageFileMeta {
            path: file.path.clone(),
            size_bytes: file.content.len(),
            digest: format!("sha256:{}", sha256_hex(&file.content)),
        })
        .collect()
}

pub(crate) fn tar_bytes(files: &[PackageFile]) -> Result<Vec<u8>, ExtensionPackageError> {
    let mut builder = TarBuilder::new(Vec::new());

    for file in files {
        append_tar_file(&mut builder, file)?;
    }

    builder.finish().context(ArchiveSnafu)?;
    builder.into_inner().context(ArchiveSnafu)
}

pub(crate) fn append_tar_file(
    builder: &mut TarBuilder<Vec<u8>>,
    file: &PackageFile,
) -> Result<(), ExtensionPackageError> {
    let mut header = Header::new_ustar();
    header.set_size(file.content.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();

    builder
        .append_data(&mut header, file.path.as_str(), file.content.as_slice())
        .context(ArchiveSnafu)
}

pub(crate) fn gzip_bytes(input: &[u8]) -> Result<Vec<u8>, ExtensionPackageError> {
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder.write_all(input).context(ArchiveSnafu)?;
    encoder.finish().context(ArchiveSnafu)
}
