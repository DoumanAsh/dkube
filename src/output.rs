//!Efficient output formatting to minimize allocations necessary for use of `tabled`

use core::{mem, marker};

///Max possible field number
pub const MAX_FIELD_COUNT: usize = 11;

///Describe `DATA` field
pub trait Field<DATA> {
    ///Returns name of the field
    fn get_key(&self) -> &'static str;
    ///Returns value of the field
    fn get_value<'a>(&self, data: &'a DATA) -> Option<&'a str>;
}

impl<DATA, F: Field<DATA> + ?Sized> Field<DATA> for &F {
    #[inline(always)]
    fn get_key(&self) -> &'static str {
        (**self).get_key()
    }

    #[inline(always)]
    fn get_value<'a>(&self, data: &'a DATA) -> Option<&'a str> {
        (**self).get_value(data)
    }
}

///CompactTable created by [Tabled]
pub type CompactTable<'a, DATA, IT, FIELD> = tabled::tables::CompactTable<
    TabledIter<'a, DATA, <IT as IntoIterator>::IntoIter, FIELD>,
    TabledDimensions<'a, DATA, IT, FIELD>
>;

///Output formatter for tabled crate
pub struct Tabled<DATA, F: Field<DATA>> {
    fields: statiki::Array<F, MAX_FIELD_COUNT>,
    _data: marker::PhantomData<DATA>
}

impl<DATA, F: Field<DATA>> Tabled<DATA, F> {
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
    pub fn data_iter<'a, IT: Iterator<Item = &'a DATA>>(&'a self, data: IT) -> TabledIter<'a, DATA, IT, F> {
        TabledIter {
            fields: self.fields.as_slice(),
            data,
            header_written: false,
            buffer: statiki::Array::new(),
        }
    }

    #[inline(always)]
    ///Creates `tabled` formatter for the provided data records
    ///
    ///`IT` must be cheap to copy IntoIterator (e.g. slice)
    ///It is recommended to rely on good iterators that implement `ExactSizedIterator` as these can
    ///implement [Iterator::nth] in efficient manner
    pub fn data_tabled<'a, IT: IntoIterator<Item = &'a DATA> + Copy>(&'a self, data: IT) -> CompactTable<'a, DATA, IT, F> {
        let data_iter = self.data_iter(data.into_iter());
        let dimension = TabledDimensions {
            fields: &self.fields,
            data,
        };
        CompactTable::with_dimension(data_iter, dimension)
    }
}

///`Dimension` implementation
pub struct TabledDimensions<'a, DATA: 'a, IT: IntoIterator<Item = &'a DATA> + Copy, F: Field<DATA>> {
    fields: &'a [F],
    data: IT
}

impl<'a, DATA: 'a, IT: IntoIterator<Item = &'a DATA> + Copy, F: Field<DATA>> tabled::grid::dimension::Dimension for TabledDimensions<'a, DATA, IT, F> {
    fn get_width(&self, column: usize) -> usize {
        use tabled::grid::util::string::get_string_width;

        match column {
            0 => self.fields.iter().map(|field| get_string_width(field.get_key())).max().unwrap_or_default(),
            idx => self.data.into_iter()
                            .nth(idx.saturating_sub(1))
                            .map(|data| self.fields.iter().map(|field| get_string_width(field.get_value(data).unwrap_or_default())).max().unwrap_or_default())
                            .unwrap_or_default()
        }
    }

    fn get_height(&self, _: usize) -> usize {
        1
    }
}

///Iterator to passs into `tabled` for purpose of printing data
pub struct TabledIter<'a, DATA: 'a, IT: Iterator<Item = &'a DATA>, F: Field<DATA>> {
    fields: &'a [F],
    data: IT,
    header_written: bool,
    buffer: statiki::Array<&'a str, MAX_FIELD_COUNT>,
}

impl<'a, DATA, IT: Iterator<Item = &'a DATA>, F: Field<DATA>> Iterator for TabledIter<'a, DATA, IT, F> {
    type Item = statiki::array::ArrayConsumer<&'a str, MAX_FIELD_COUNT>;
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
            $typ:ident$(<$lifetime:lifetime>)?$(.$field:ident)+)+
        )+
    ) => {
        $(
            #[derive(Copy, Clone)]
            $(#[$doc])*
            pub struct $name;

            $(
                impl Field<$crate::kube::$typ$(<$lifetime>)*> for $name {
                    #[inline(always)]
                    fn get_key(&self) -> &'static str {
                        stringify!($name)
                    }

                    #[inline(always)]
                    fn get_value<'a>(&self, data: &'a $crate::kube::$typ) -> Option<&'a str> {
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
            $typ:ident$(<$lifetime:lifetime>)?$(.$field:ident)+)+
        )+
    ) => {
        $(
            #[derive(Copy, Clone)]
            $(#[$doc])*
            pub struct $name;

            $(
                impl Field<$crate::kube::$typ$(<$lifetime>)*> for $name {
                    #[inline(always)]
                    fn get_key(&self) -> &'static str {
                        stringify!($name)
                    }

                    #[inline(always)]
                    fn get_value<'a>(&self, data: &'a $crate::kube::$typ) -> Option<&'a str> {
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
        impl Name for Pod.name PodContainer.name PodContainerFullRef<'_>.pod.name
        ///Namespace accessor
        impl Namespace for Pod.namespace PodContainerFullRef<'_>.pod.namespace
        ///Status accessor
        impl Status for PodContainer.status PodContainerFullRef<'_>.container.status
        ///Image accessor
        impl Image for PodContainer.image PodContainerFullRef<'_>.container.image
    );

    impl_opt_field!(
        ///Node accessor
        impl Node for Pod.node_name PodContainerFullRef<'_>.pod.node_name
        ///IP accessor
        impl Ip for Pod.pod_ip PodContainerFullRef<'_>.pod.pod_ip
        ///ImageId accessor
        impl ImageId for PodContainer.image_id PodContainerFullRef<'_>.container.image_id
        ///CPU accessor
        impl CPU for PodContainer.resources.requests.cpu PodContainerFullRef<'_>.container.resources.requests.cpu
        ///Memory accessor
        impl MEM for PodContainer.resources.requests.memory PodContainerFullRef<'_>.container.resources.requests.memory
        #[allow(non_camel_case_types)]
        ///Max CPU accessor
        impl MAX_CPU for PodContainer.resources.limits.cpu PodContainerFullRef<'_>.container.resources.limits.cpu
        #[allow(non_camel_case_types)]
        ///Max Memory accessor
        impl MAX_MEM for PodContainer.resources.limits.memory PodContainerFullRef<'_>.container.resources.limits.memory
    );
}
