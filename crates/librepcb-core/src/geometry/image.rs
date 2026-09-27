//! Port of libs/librepcb/core/geometry/image.{h,cpp}.
//!
//! Not ported: `Image::tryLoad()` (depends on `QImage`/`QSvgRenderer`, to be
//! implemented in the rendering layer).

use super::{impl_uuid, object_list, property};
use crate::serialization::{
    self, DeserializeObject, FromSExpression, List, SExpression, SerializeObject, ToSExpression,
};
use crate::types::{Angle, FileProofName, Length, Point, PositiveLength, UnsignedLength, Uuid};

/// An image (file in the library element / project directory) placed in a
/// symbol, schematic etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Image {
    uuid: Uuid,
    file_name: FileProofName,
    position: Point,
    rotation: Angle,
    width: PositiveLength,
    height: PositiveLength,
    border_width: Option<UnsignedLength>,
}

impl Image {
    /// The supported file extensions (lowercase, without dot).
    ///
    /// This list is part of the file format specification; any change
    /// requires bumping the file format version. Other formats must be
    /// converted to one of these when adding images.
    pub const SUPPORTED_EXTENSIONS: [&'static str; 3] = ["jpg", "png", "svg"];

    /// Creates an image.
    pub fn new(
        uuid: Uuid,
        file_name: FileProofName,
        position: Point,
        rotation: Angle,
        width: PositiveLength,
        height: PositiveLength,
        border_width: Option<UnsignedLength>,
    ) -> Self {
        Self {
            uuid,
            file_name,
            position,
            rotation,
            width,
            height,
            border_width,
        }
    }

    property!(
        /// Returns the file name.
        ref file_name: FileProofName, set_file_name
    );
    property!(
        /// Returns the position (bottom left corner).
        copy position: Point, set_position
    );
    property!(
        /// Returns the rotation around the position.
        copy rotation: Angle, set_rotation
    );
    property!(
        /// Returns the width.
        copy width: PositiveLength, set_width
    );
    property!(
        /// Returns the height.
        copy height: PositiveLength, set_height
    );
    property!(
        /// Returns the border width (`None` = no border).
        copy border_width: Option<UnsignedLength>, set_border_width
    );

    /// Returns the file name without the last extension.
    pub fn file_basename(&self) -> &str {
        self.file_name
            .rsplit_once('.')
            .map_or(self.file_name.as_str(), |(base, _)| base)
    }

    /// Returns the last extension of the file name (or the whole name if it
    /// contains no dot).
    pub fn file_extension(&self) -> &str {
        self.file_name
            .rsplit_once('.')
            .map_or(self.file_name.as_str(), |(_, ext)| ext)
    }

    /// Returns the center of the (rotated) image.
    pub fn center(&self) -> Point {
        self.position
            + Point::new(self.width / 2, self.height / 2).rotated(self.rotation, Point::ORIGIN)
    }
}

impl_uuid!(Image);

object_list!(
    /// List of [`Image`]s.
    ImageList,
    ImageListTag,
    Image,
    "image"
);

/// Serializes the border width as length or `none`.
fn serialize_border(border: Option<UnsignedLength>) -> SExpression {
    border.map_or_else(|| SExpression::token("none"), |b| b.to_sexpression())
}

fn deserialize_border(node: &SExpression) -> serialization::Result<Option<UnsignedLength>> {
    match node.value()? {
        "none" => Ok(None),
        _ => Ok(Some(UnsignedLength::new(Length::from_sexpression(node)?)?)),
    }
}

impl SerializeObject for Image {
    fn serialize(&self, root: &mut List) {
        root.append_value(&self.uuid);
        root.append_child("file", &self.file_name);
        root.ensure_line_break();
        self.position.serialize(root.append_list("position"));
        root.append_child("rotation", &self.rotation);
        root.append_child("width", &self.width);
        root.append_child("height", &self.height);
        root.ensure_line_break();
        root.append_child("border", &serialize_border(self.border_width));
        root.ensure_line_break();
    }
}

impl DeserializeObject for Image {
    fn deserialize(node: &SExpression) -> serialization::Result<Self> {
        Ok(Self {
            uuid: node.child_value("@0")?,
            file_name: node.child_value("file/@0")?,
            position: Point::deserialize(node.required_child("position")?)?,
            rotation: node.child_value("rotation/@0")?,
            width: node.child_value("width/@0")?,
            height: node.child_value("height/@0")?,
            border_width: deserialize_border(node.required_child("border/@0")?)?,
        })
    }
}
