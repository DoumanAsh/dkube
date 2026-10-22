//!Efficient output formatting to minimize allocations necessary for use of `tabled`

use core::{mem, marker};

///Max possible field number
pub const MAX_FIELD_COUNT: usize = 11;

//TODO: This is a bit dumb but Copy basically is hack to get cheap ref type as parameter to allow PodContainerFullRef<'_> or plain reference
//      AsRef cannot return the same lifetime as 'data, so either I create new trait or just use Copy
//      Maybe there is some other idea, but if this works then who cares...
///Describe `DATA` field
pub trait Field<'data, DATA: Copy + 'data> {
    ///Returns name of the field
    fn get_key(&self) -> &'static str;
    ///Returns value of the field
    fn get_value(&self, data: DATA) -> Option<&'data str>;
}

impl<'data, DATA: Copy + 'data, F: Field<'data, DATA> + ?Sized> Field<'data, DATA> for &F {
    #[inline(always)]
    fn get_key(&self) -> &'static str {
        (**self).get_key()
    }

    #[inline(always)]
    fn get_value(&self, data: DATA) -> Option<&'data str> {
        (**self).get_value(data)
    }
}

///CompactTable created by [Tabled]
pub type CompactTable<'a, 'data, DATA, IT, FIELD> = tabled::tables::CompactTable<
    TabledIter<'a, 'data, DATA, <IT as IntoIterator>::IntoIter, FIELD>,
    TabledDimensions<'a, 'data, DATA, IT, FIELD>
>;

///Output formatter for tabled crate
pub struct Tabled<'data, DATA: Copy + 'data, F: Field<'data, DATA>> {
    fields: statiki::Array<F, MAX_FIELD_COUNT>,
    _data: marker::PhantomData<&'data DATA>
}

impl<'data, DATA: Copy + 'data, F: Field<'data, DATA>> Tabled<'data, DATA, F> {
    #[inline(always)]
    ///Creates new empty [Tabled]
    pub const fn new() -> Self {
        Self {
            fields: statiki::Array::new(),
            _data: marker::PhantomData,
        }
    }

    ///Add header, with overflow if there is more columns than expected (i.e. duplicates)
    pub const fn with_header(mut self, header: F) -> Self {
        if self.fields.len() < self.fields.capacity() {
            unsafe {
                self.fields.push_unchecked(header);
                self
            }
        } else {
            panic!("Table expects at most 11 columns, avoid duplicates");
        }
    }

    #[inline(always)]
    ///Returns iterator over header names and  `data` values as iterator of iterators [TabledIter]
    pub fn data_iter<'a, IT: Iterator<Item = DATA>>(&'a self, data: IT) -> TabledIter<'a, 'data, DATA, IT, F> {
        TabledIter {
            fields: self.fields.as_slice(),
            data,
            header_written: false,
            buffer: statiki::Array::new(),
            _data: marker::PhantomData,
        }
    }

    #[inline(always)]
    ///Creates `tabled` formatter for the provided data records
    ///
    ///`IT` must be cheap to copy IntoIterator (e.g. slice)
    ///It is recommended to rely on good iterators that implement `ExactSizedIterator` as these can
    ///implement [Iterator::nth] in efficient manner
    pub fn data_tabled<'a, IT: IntoIterator<Item = DATA> + Copy>(&'a self, data: IT) -> CompactTable<'a, 'data, DATA, IT, F> {
        let data_iter = self.data_iter(data.into_iter());
        let dimension = TabledDimensions {
            fields: &self.fields,
            data,
            _data: marker::PhantomData,
        };
        CompactTable::with_dimension(data_iter, dimension).columns(self.fields.len())
    }
}

///`Dimension` implementation
pub struct TabledDimensions<'a, 'data, DATA: Copy + 'data, IT: IntoIterator + Copy, F: Field<'data, DATA>> {
    fields: &'a [F],
    data: IT,
    _data: core::marker::PhantomData<&'data DATA>,
}

impl<'a, 'data, DATA: Copy + 'data, IT: IntoIterator<Item = DATA> + Copy, F: Field<'data, DATA>> tabled::grid::dimension::Dimension for TabledDimensions<'a, 'data, DATA, IT, F> {
    fn get_width(&self, column: usize) -> usize {
        use tabled::grid::util::string::get_string_width;

        let result = match column {
            0 => self.fields.iter().map(|field| get_string_width(field.get_key())).max().unwrap_or_default(),
            idx => self.data.into_iter()
                            .nth(idx.saturating_sub(1))
                            .map(|data| self.fields.iter().map(|field| get_string_width(field.get_value(data).unwrap_or_default())).max().unwrap_or_default())
                            .unwrap_or_default()
        };
        //tabled needs at least 2 bytes of padding
        result.saturating_add(2)
    }

    fn get_height(&self, _: usize) -> usize {
        1
    }
}

///Iterator to passs into `tabled` for purpose of printing data
pub struct TabledIter<'a, 'data, DATA: Copy + 'data, IT: Iterator, F: Field<'data, DATA>> {
    fields: &'a [F],
    data: IT,
    header_written: bool,
    buffer: statiki::Array<&'data str, MAX_FIELD_COUNT>,
    _data: core::marker::PhantomData<&'data DATA>,
}

impl<'a, 'data, DATA: Copy + 'data, IT: Iterator<Item = DATA>, F: Field<'data, DATA>> Iterator for TabledIter<'a, 'data, DATA, IT, F> {
    type Item = statiki::array::ArrayConsumer<&'data str, MAX_FIELD_COUNT>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.header_written {
            let data = self.data.next()?;
            for field in self.fields {
                let data = field.get_value(data).unwrap_or_default();
                unsafe {
                    self.buffer.push_unchecked(data);
                }
            }
        } else {
            self.header_written = true;
            for field in self.fields {
                unsafe {
                    self.buffer.push_unchecked(field.get_key());
                }
            }
        }

        Some(mem::take(&mut self.buffer).into_iter())
    }
}

macro_rules! impl_field {
    (
        $(
        $(#[$doc:meta])*
        impl $name:ident for $(
            $(&$ref:lifetime)?$typ:ident$(<$lifetime:lifetime>)?$(.$field:ident)+)+
        )+
    ) => {
        $(
            #[derive(Copy, Clone)]
            $(#[$doc])*
            pub struct $name;

            $(
                impl<'data> Field<'data, $(&$ref)*$crate::kube::$typ$(<$lifetime>)*> for $name {
                    #[inline(always)]
                    fn get_key(&self) -> &'static str {
                        stringify!($name)
                    }

                    #[inline(always)]
                    fn get_value(&self, data: $(&$ref)*$crate::kube::$typ$(<$lifetime>)*) -> Option<&'data str> {
                        Some(&data$(.$field)+)
                    }
                }
            )+
        )+
    };
}

macro_rules! impl_opt_field {
    (
        $(
        $(#[$doc:meta])*
        impl $name:ident for $(
            $(&$ref:lifetime)?$typ:ident$(<$lifetime:lifetime>)?$(.$field:ident)+)+
        )+
    ) => {
        $(
            #[derive(Copy, Clone)]
            $(#[$doc])*
            pub struct $name;

            $(
                impl<'data> Field<'data, $(&$ref)*$crate::kube::$typ$(<$lifetime>)*> for $name {
                    #[inline(always)]
                    fn get_key(&self) -> &'static str {
                        stringify!($name)
                    }

                    #[inline(always)]
                    fn get_value(&self, data: $(&$ref)*$crate::kube::$typ$(<$lifetime>)*) -> Option<&'data str> {
                        match &data$(.$field)+ {
                            Some(value) => Some(value),
                            None => None,
                        }
                    }
                }
            )+
        )+
    };
}

///Output fields
pub mod fields {
    use super::Field;

    impl_field!(
        ///Name accessor
        impl Name for &'data Pod.name &'data PodContainer.name PodContainerFullRef<'data>.pod.name
        ///Namespace accessor
        impl Namespace for &'data Pod.namespace PodContainerFullRef<'data>.pod.namespace
        ///Status accessor
        impl Status for &'data PodContainer.status PodContainerFullRef<'data>.container.status
        ///Image accessor
        impl Image for &'data PodContainer.image PodContainerFullRef<'data>.container.image
    );

    impl_opt_field!(
        ///Node accessor
        impl Node for &'data Pod.node_name PodContainerFullRef<'data>.pod.node_name
        ///IP accessor
        impl Ip for &'data Pod.pod_ip PodContainerFullRef<'data>.pod.pod_ip
        ///ImageId accessor
        impl ImageId for &'data PodContainer.image_id PodContainerFullRef<'data>.container.image_id
        ///CPU accessor
        impl CPU for &'data PodContainer.resources.requests.cpu PodContainerFullRef<'data>.container.resources.requests.cpu
        ///Memory accessor
        impl MEM for &'data PodContainer.resources.requests.memory PodContainerFullRef<'data>.container.resources.requests.memory
        #[allow(non_camel_case_types)]
        ///Max CPU accessor
        impl MAX_CPU for &'data PodContainer.resources.limits.cpu PodContainerFullRef<'data>.container.resources.limits.cpu
        #[allow(non_camel_case_types)]
        ///Max Memory accessor
        impl MAX_MEM for &'data PodContainer.resources.limits.memory PodContainerFullRef<'data>.container.resources.limits.memory
    );
}
